//! Branch — the working context for all read/write operations.
//!
//! A branch knows its identity, its ancestry chain, and holds a clone
//! of Storage for database access. All domain operations go through a branch.

use uuid::Uuid;

use serde_json::{Map, Value};

use crate::io::DbError;
use crate::io::slug::Slug;
use crate::store::branch_entry::{BranchEntry, BranchKey};
use crate::store::storage::Storage;
use crate::store::transaction_writer::TransactionWriter;

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
pub struct Branch {
    storage: Storage,
    branch: Slug,
    ancestry: Vec<AncestryEntry>,
}

impl Branch {
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

    /// Precomputed ancestry chain.
    pub fn ancestry(&self) -> &[AncestryEntry] {
        &self.ancestry
    }

    /// Start a new transaction on this branch.
    pub fn transaction(&self, meta: Map<String, Value>) -> Result<TransactionWriter, DbError> {
        let batch = self.storage.db.batch();
        Ok(TransactionWriter::new(self.branch.clone(), meta, batch))
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
    use crate::store::branch_entry::{BranchEntry, BranchKey, BranchValue};

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
        let branch = Branch::main(storage);
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

        let branch = Branch::open(storage, child_slug.clone()).unwrap();
        assert_eq!(branch.id(), &child_slug);
        assert_eq!(branch.ancestry().len(), 2);
        assert_eq!(branch.ancestry()[0].branch, child_slug);
        assert_eq!(branch.ancestry()[0].branch_point_tx, Uuid::max());
        assert_eq!(branch.ancestry()[1].branch, main);
        assert_eq!(branch.ancestry()[1].branch_point_tx, branch_point);
    }

    #[test]
    fn open_main_by_slug() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let main = main_branch_slug();
        let branch = Branch::open(storage, main.clone()).unwrap();
        assert_eq!(branch.id(), &main);
        assert_eq!(branch.ancestry().len(), 1);
    }
}
