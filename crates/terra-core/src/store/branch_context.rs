//! BranchContext — the working context for all read/write operations.
//!
//! A branch context knows its identity, its ancestry chain, and holds a clone
//! of Storage for database access. All domain operations go through a branch context.

use uuid::Uuid;

use crate::io::key_prefix::KeyBound;
use crate::io::slug::Slug;
use crate::io::storage_key::StorageKey;
use crate::io::{DbError, DbItem};
use crate::store::entry::branch::{BranchEntry, BranchKey};
use crate::store::entry::transaction::{TransactionEntry, TransactionKey};
use crate::store::storage::Storage;
use crate::store::versioned_key::VersionedKey;

#[cfg(test)]
use crate::domain::transaction::Transaction;
#[cfg(test)]
use crate::domain::tx_meta::{time_from_uuid, TxMeta};
#[cfg(test)]
use crate::store::entry::transaction::TransactionValue;

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

/// A single step in the ancestry walk: branch + optional tx upper bound.
///
/// `upper_tx: None` means unbounded (current branch head).
/// `upper_tx: Some(tx)` means only records up to that tx_id.
#[derive(Debug, Clone)]
pub struct BranchScope {
    pub branch: Slug,
    pub upper_tx: Option<Uuid>,
}

impl BranchScope {
    /// Apply this scope to a key bound: override branch, optionally cap tx_id.
    pub fn apply_bound<K>(&self, bound: &KeyBound<K>) -> KeyBound<K>
    where
        K: VersionedKey + Clone,
    {
        let mut result = bound
            .clone()
            .with_prefix(|k| k.set_branch(self.branch.clone()));
        if let Some(tx) = self.upper_tx {
            result = result.with_upper(|k| k.set_tx_id(tx));
        }
        result
    }
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

    /// Iterate scopes: current branch (unbounded) then ancestors (tx-bounded).
    pub fn scopes(&self) -> impl Iterator<Item = BranchScope> + '_ {
        std::iter::once(BranchScope {
            branch: self.branch.clone(),
            upper_tx: None,
        })
        .chain(self.ancestry.iter().map(|a| BranchScope {
            branch: a.branch.clone(),
            upper_tx: Some(a.branch_point_tx),
        }))
    }

    /// Iterate scopes with an explicit tx upper bound on the current branch.
    pub fn scopes_at(&self, at_tx: Uuid) -> impl Iterator<Item = BranchScope> + '_ {
        std::iter::once(BranchScope {
            branch: self.branch.clone(),
            upper_tx: Some(at_tx),
        })
        .chain(self.ancestry.iter().map(|a| BranchScope {
            branch: a.branch.clone(),
            upper_tx: Some(a.branch_point_tx),
        }))
    }

    /// Check if any record exists, walking the ancestry chain.
    pub fn exists<T>(&self, bound: &KeyBound<T::Key>) -> Result<bool, DbError>
    where
        T: DbItem,
        T::Key: VersionedKey + Clone,
    {
        for scope in self.scopes() {
            if self.storage.exists::<T>(&scope.apply_bound(bound))? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Get the latest version, walking the ancestry chain.
    pub fn get_latest<T>(&self, bound: &KeyBound<T::Key>) -> Result<Option<T>, DbError>
    where
        T: DbItem,
        T::Key: VersionedKey + Clone,
    {
        for scope in self.scopes() {
            if let Some(found) = self.storage.get_latest::<T>(&scope.apply_bound(bound))? {
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
                "branch depth exceeds maximum of {}",
                max_depth
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
        let bound = TransactionKey::bound().with_prefix(|k| k.branch = self.branch.clone());
        let entry = self.storage.get_latest::<TransactionEntry>(&bound)?;
        Ok(entry.map(|e| e.key.tx_id))
    }

    /// Commit a transaction on this branch (test helper).
    #[cfg(test)]
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
                reasoning: None,
                time: time_from_uuid(tx_id),
                status: None,
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

    #[test]
    fn scopes_main_branch() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);

        let scopes: Vec<BranchScope> = branch.scopes().collect();
        assert_eq!(scopes.len(), 1);
        assert_eq!(scopes[0].branch, main_branch_slug());
        assert!(scopes[0].upper_tx.is_none());
    }

    #[test]
    fn scopes_child_branch() {
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
        let scopes: Vec<BranchScope> = branch.scopes().collect();
        assert_eq!(scopes.len(), 2);
        assert_eq!(scopes[0].branch, child_slug);
        assert!(scopes[0].upper_tx.is_none());
        assert_eq!(scopes[1].branch, main);
        assert_eq!(scopes[1].upper_tx, Some(branch_point));
    }

    #[test]
    fn scopes_at_bounds_current_branch() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();

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
        let at = Uuid::now_v7();
        let scopes: Vec<BranchScope> = branch.scopes_at(at).collect();
        assert_eq!(scopes.len(), 2);
        assert_eq!(scopes[0].branch, child_slug);
        assert_eq!(scopes[0].upper_tx, Some(at));
        assert_eq!(scopes[1].upper_tx, Some(branch_point));
    }
}
