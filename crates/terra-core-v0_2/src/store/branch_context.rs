//! BranchContext — the working context for all read/write operations.
//!
//! A branch context knows its identity, its ancestry chain, and holds a clone
//! of Storage for database access. All domain operations go through a branch context.

use uuid::Uuid;

use crate::domain::transaction::Transaction;
use crate::domain::tx_meta::TxMeta;
use crate::io::key_prefix::KeyBound;
use crate::io::slug::Slug;
use crate::io::storage_key::StorageKey;
use crate::io::{DbError, DbItem};
use crate::store::entry::branch::{BranchEntry, BranchKey};
use crate::store::entry::transaction::{TransactionEntry, TransactionKey, TransactionValue};
use crate::store::storage::Storage;
use crate::store::versioned_key::VersionedKey;

/// Main branch slug — always exists implicitly.
pub fn main_branch_slug() -> Slug {
    Slug::new_unchecked("main")
}

/// Precomputed ancestry entry: branch_id + upper tx bound.
#[derive(Debug, Clone)]
pub struct AncestryEntry {
    pub branch: Slug,
    pub branch_point_tx: Uuid,
}

/// Working context bound to a specific branch.
#[derive(Clone)]
pub struct BranchContext {
    storage: Storage,
    branch: Slug,
    ancestry: Vec<AncestryEntry>,
}

impl BranchContext {
    /// Open the main branch.
    pub fn main(storage: Storage) -> Self {
        let main = main_branch_slug();
        Self {
            storage,
            branch: main,
            ancestry: vec![],
        }
    }

    /// Open a branch by slug. Loads the branch record and computes ancestry.
    pub fn open(storage: Storage, branch: Slug) -> Result<Self, DbError> {
        if branch == main_branch_slug() {
            return Ok(Self::main(storage));
        }

        let max_depth = storage.config().max_branch_depth;
        let ancestry = Self::compute_ancestry(&storage, branch.clone(), max_depth)?;
        Ok(Self {
            storage,
            branch,
            ancestry,
        })
    }

    /// Branch slug.
    pub fn id(&self) -> &Slug {
        &self.branch
    }

    /// Access the underlying storage (crate-internal).
    pub(crate) fn storage(&self) -> &Storage {
        &self.storage
    }

    /// Precomputed ancestry chain.
    pub fn ancestry(&self) -> &[AncestryEntry] {
        &self.ancestry
    }

    /// Check if any record exists, walking the ancestry chain.
    ///
    /// Checks current branch (unbounded), then ancestors with tx bounds.
    pub fn exists<T>(&self, bound: &KeyBound<T::Key>) -> Result<bool, DbError>
    where
        T: DbItem,
        T::Key: VersionedKey + Clone,
    {
        if self.storage.exists::<T>(bound)? {
            return Ok(true);
        }
        for entry in &self.ancestry {
            let bounded = bound.clone()
                .with_prefix(|k| k.set_branch(entry.branch.clone()))
                .with_upper(|k| k.set_tx_id(entry.branch_point_tx));
            if self.storage.exists::<T>(&bounded)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Get the latest version, walking the ancestry chain.
    ///
    /// Checks current branch (unbounded), then ancestors with tx bounds.
    pub fn get_latest<T>(&self, bound: &KeyBound<T::Key>) -> Result<Option<T>, DbError>
    where
        T: DbItem,
        T::Key: VersionedKey + Clone,
    {
        if let Some(found) = self.storage.get_latest::<T>(bound)? {
            return Ok(Some(found));
        }
        for entry in &self.ancestry {
            let bounded = bound.clone()
                .with_prefix(|k| k.set_branch(entry.branch.clone()))
                .with_upper(|k| k.set_tx_id(entry.branch_point_tx));
            if let Some(found) = self.storage.get_latest::<T>(&bounded)? {
                return Ok(Some(found));
            }
        }
        Ok(None)
    }

    /// Build a child branch context without reading from storage.
    ///
    /// The child inherits this branch's ancestry plus an entry for this branch
    /// at the given branch point. Use this when the child's BranchEntry
    /// hasn't been committed yet (e.g. inside a composite command).
    pub fn child(&self, slug: Slug, branch_point_tx: Uuid) -> Result<Self, DbError> {
        let max_depth = self.storage.config().max_branch_depth;
        if self.ancestry.len() + 1 > max_depth {
            return Err(DbError::Storage(format!(
                "branch depth exceeds maximum of {}", max_depth
            )));
        }
        let mut ancestry = vec![AncestryEntry {
            branch: self.branch.clone(),
            branch_point_tx,
        }];
        ancestry.extend(self.ancestry.iter().cloned());
        Ok(Self {
            storage: self.storage.clone(),
            branch: slug,
            ancestry,
        })
    }

    /// Return the tx_id of the latest transaction on this branch (not walking ancestry).
    pub fn head_tx(&self) -> Result<Option<Uuid>, DbError> {
        let bound = TransactionKey::bound()
            .with_prefix(|k| k.branch = self.branch.clone());
        let entry = self.storage.get_latest::<TransactionEntry>(&bound)?;
        Ok(entry.map(|e| e.key.tx_id))
    }

    /// Commit a transaction on this branch.
    pub fn commit(&self, tx: Transaction) -> Result<Transaction<TxMeta>, DbError> {
        let tx_id = Uuid::now_v7();

        let entry = TransactionEntry {
            key: TransactionKey {
                branch: self.branch.clone(),
                tx_id,
            },
            value: TransactionValue {
                meta: tx.meta.clone(),
            },
        };

        let mut batch = self.storage.batch();
        batch.put(&entry)?;
        batch.commit()?;

        Ok(Transaction {
            meta: tx.meta,
            context: TxMeta {
                tx_id,
                branch: self.branch.clone(),
            },
        })
    }

    fn compute_ancestry(
        storage: &Storage,
        branch: Slug,
        max_depth: usize,
    ) -> Result<Vec<AncestryEntry>, DbError> {
        let main = main_branch_slug();
        let mut chain = Vec::new();
        let mut current_id = branch;

        for _ in 0..max_depth {
            let key = BranchKey { branch: current_id };
            let entry = storage
                .get::<BranchEntry>(&key)?
                .ok_or_else(|| DbError::Storage(format!("branch not found: {}", key.branch)))?;

            let parent_slug: Slug = entry
                .value
                .parent_branch_slug
                .parse()
                .map_err(|e: crate::io::slug::SlugError| DbError::Storage(e.to_string()))?;
            let branch_point = entry.value.created_from_tx;

            chain.push(AncestryEntry {
                branch: parent_slug.clone(),
                branch_point_tx: branch_point,
            });

            if parent_slug == main {
                break;
            }
            current_id = parent_slug;
        }

        Ok(chain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProjectConfig;
    use crate::store::entry::branch::{BranchEntry, BranchKey, BranchValue};
    use std::sync::Arc;

    fn test_config() -> Arc<ProjectConfig> {
        Arc::new(
            ProjectConfig::builder()
                .data_dir("./data".into())
                .schema_path("./schema.yaml".into())
                .build(),
        )
    }

    #[test]
    fn main_branch() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);
        let main = main_branch_slug();

        assert_eq!(branch.id(), &main);
        assert!(branch.ancestry().is_empty());
    }

    #[test]
    fn open_child_branch() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = main_branch_slug();

        let child_slug: Slug = "child".parse().unwrap();
        let branch_point = Uuid::now_v7();

        let entry = BranchEntry {
            key: BranchKey {
                branch: child_slug.clone(),
            },
            value: BranchValue {
                slug: "child".into(),
                meta: serde_json::Map::new(),
                parent_branch_slug: "main".into(),
                created_from_tx: branch_point,
            },
        };
        let mut batch = storage.db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let branch = BranchContext::open(storage, child_slug.clone()).unwrap();
        assert_eq!(branch.id(), &child_slug);
        assert_eq!(branch.ancestry().len(), 1);
        assert_eq!(branch.ancestry()[0].branch, main);
        assert_eq!(branch.ancestry()[0].branch_point_tx, branch_point);
    }

    #[test]
    fn commit_transaction() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage.clone());

        let mut meta = serde_json::Map::new();
        meta.insert("reasoning".into(), serde_json::json!("test"));

        let tx = Transaction::new(meta);
        let committed = branch.commit(tx).unwrap();

        assert_eq!(committed.context.branch, main_branch_slug());
        assert_eq!(committed.meta["reasoning"], "test");

        // Verify written to DB
        let key = TransactionKey {
            branch: main_branch_slug(),
            tx_id: committed.context.tx_id,
        };
        let found = storage.db.get::<TransactionEntry>(&key).unwrap().unwrap();
        assert_eq!(found.value.meta["reasoning"], "test");
    }

    #[test]
    fn head_tx_returns_latest() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let mut meta = serde_json::Map::new();
        meta.insert("reasoning".into(), serde_json::json!("first"));
        let tx1 = branch.commit(Transaction::new(meta)).unwrap();

        let mut meta2 = serde_json::Map::new();
        meta2.insert("reasoning".into(), serde_json::json!("second"));
        let tx2 = branch.commit(Transaction::new(meta2)).unwrap();

        let head = branch.head_tx().unwrap().unwrap();
        assert_eq!(head, tx2.context.tx_id);
        assert_ne!(head, tx1.context.tx_id);
    }

    #[test]
    fn head_tx_empty_branch() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        assert!(branch.head_tx().unwrap().is_none());
    }

    #[test]
    fn open_main_by_slug() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = main_branch_slug();
        let branch = BranchContext::open(storage, main.clone()).unwrap();
        assert_eq!(branch.id(), &main);
        assert!(branch.ancestry().is_empty());
    }
}
