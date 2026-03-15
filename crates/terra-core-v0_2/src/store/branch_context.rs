//! BranchContext — the working context for all read/write operations.
//!
//! A branch context knows its identity, its ancestry chain, and holds a clone
//! of Storage for database access. All domain operations go through a branch context.

use uuid::Uuid;

use crate::domain::transaction::Transaction;
use crate::domain::tx_meta::TxMeta;
use crate::io::{DbError, DbItem, ValidPrefix};
use crate::io::slug::Slug;
use crate::store::entry::branch::{BranchEntry, BranchKey};
use crate::store::entry::transaction::{TransactionEntry, TransactionKey, TransactionValue};
use crate::store::storage::Storage;
use crate::store::versioned_key::VersionedPrefix;

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
            branch: main.clone(),
            ancestry: vec![AncestryEntry {
                branch: main,
                branch_point_tx: Uuid::max(),
            }],
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

    /// Check if any record exists for the given prefix.
    pub fn exists<T: DbItem>(&self, prefix: &(impl VersionedPrefix + ValidPrefix<T>)) -> Result<bool, DbError> {
        let mut iter = self.storage.db.scan::<T>(prefix)?;
        Ok(iter.next().is_some())
    }

    /// Get the latest version (highest tx_id) for the given prefix.
    pub fn get_latest<T: DbItem>(&self, prefix: &(impl VersionedPrefix + ValidPrefix<T>)) -> Result<Option<T>, DbError> {
        let mut iter = self.storage.db.scan_rev::<T>(prefix)?;
        match iter.next() {
            Some(result) => Ok(Some(result?)),
            None => Ok(None),
        }
    }

    /// Commit a transaction on this branch.
    pub fn commit(&self, tx: Transaction) -> Result<Transaction<TxMeta>, DbError> {
        let tx_id = Uuid::now_v7();

        let entry = TransactionEntry {
            key: TransactionKey {
                branch: self.branch.clone(),
                tx_id,
            },
            value: TransactionValue { meta: tx.meta.clone() },
        };

        let mut batch = self.storage.db.batch();
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

    fn compute_ancestry(storage: &Storage, branch: Slug, max_depth: usize) -> Result<Vec<AncestryEntry>, DbError> {
        let main = main_branch_slug();
        let mut chain = vec![AncestryEntry {
            branch: branch.clone(),
            branch_point_tx: Uuid::max(),
        }];

        let mut current_id = branch;

        for _ in 0..max_depth {
            let key = BranchKey { branch: current_id };
            let entry = storage.db.get::<BranchEntry>(&key)?
                .ok_or_else(|| DbError::Storage(format!("branch not found: {}", key.branch)))?;

            let parent_slug: Slug = entry.value.parent_branch_slug.parse()
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
    use std::sync::Arc;
    use super::*;
    use crate::config::ProjectConfig;
    use crate::store::entry::branch::{BranchEntry, BranchKey, BranchValue};

    fn test_config() -> Arc<ProjectConfig> {
        Arc::new(ProjectConfig::builder()
            .data_dir("./data".into())
            .schema_path("./schema.yaml".into())
            .build())
    }

    #[test]
    fn main_branch() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = BranchContext::main(storage);
        let main = main_branch_slug();

        assert_eq!(branch.id(), &main);
        assert_eq!(branch.ancestry().len(), 1);
        assert_eq!(branch.ancestry()[0].branch, main);
        assert_eq!(branch.ancestry()[0].branch_point_tx, Uuid::max());
    }

    #[test]
    fn open_child_branch() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = main_branch_slug();

        let child_slug: Slug = "child".parse().unwrap();
        let branch_point = Uuid::now_v7();

        let entry = BranchEntry {
            key: BranchKey { branch: child_slug.clone() },
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
        assert_eq!(branch.ancestry().len(), 2);
        assert_eq!(branch.ancestry()[0].branch, child_slug);
        assert_eq!(branch.ancestry()[0].branch_point_tx, Uuid::max());
        assert_eq!(branch.ancestry()[1].branch, main);
        assert_eq!(branch.ancestry()[1].branch_point_tx, branch_point);
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
    fn open_main_by_slug() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = main_branch_slug();
        let branch = BranchContext::open(storage, main.clone()).unwrap();
        assert_eq!(branch.id(), &main);
        assert_eq!(branch.ancestry().len(), 1);
    }
}
