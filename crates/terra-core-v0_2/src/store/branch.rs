//! Branch — the working context for all read/write operations.
//!
//! A branch knows its identity, its ancestry chain, and holds a clone
//! of Storage for database access. All domain operations go through a branch.

use uuid::Uuid;

use crate::io::DbError;
use crate::store::branch_entry::{BranchEntry, BranchKey};
use crate::store::storage::Storage;

/// Main branch has a nil UUID and always exists implicitly.
pub const MAIN_BRANCH: Uuid = Uuid::nil();

/// Precomputed ancestry entry: branch_id + upper tx bound.
#[derive(Debug, Clone)]
pub struct AncestryEntry {
    pub branch_id: Uuid,
    pub branch_point_tx: Uuid,
}

/// Working context bound to a specific branch.
#[derive(Clone)]
pub struct Branch {
    storage: Storage,
    branch_id: Uuid,
    ancestry: Vec<AncestryEntry>,
}

impl Branch {
    /// Open the main branch.
    pub fn main(storage: Storage) -> Self {
        Self {
            storage,
            branch_id: MAIN_BRANCH,
            ancestry: vec![AncestryEntry {
                branch_id: MAIN_BRANCH,
                branch_point_tx: Uuid::max(),
            }],
        }
    }

    /// Open a branch by ID. Loads the branch record and computes ancestry.
    pub fn open(storage: Storage, branch_id: Uuid) -> Result<Self, DbError> {
        if branch_id == MAIN_BRANCH {
            return Ok(Self::main(storage));
        }

        let max_depth = storage.config().max_branch_depth;
        let ancestry = Self::compute_ancestry(&storage, branch_id, max_depth)?;
        Ok(Self {
            storage,
            branch_id,
            ancestry,
        })
    }

    /// Branch UUID.
    pub fn id(&self) -> Uuid {
        self.branch_id
    }

    /// Precomputed ancestry chain.
    pub fn ancestry(&self) -> &[AncestryEntry] {
        &self.ancestry
    }

    fn compute_ancestry(storage: &Storage, branch_id: Uuid, max_depth: usize) -> Result<Vec<AncestryEntry>, DbError> {
        let mut chain = vec![AncestryEntry {
            branch_id,
            branch_point_tx: Uuid::max(),
        }];

        let mut current_id = branch_id;

        for _ in 0..max_depth {
            let key = BranchKey { branch_id: current_id };
            let entry = storage.db.get::<BranchEntry>(&key)?
                .ok_or_else(|| DbError::Storage(format!("branch not found: {}", current_id)))?;

            let parent_id = entry.value.parent_branch_id;
            let branch_point = entry.value.created_from_tx;

            chain.push(AncestryEntry {
                branch_id: parent_id,
                branch_point_tx: branch_point,
            });

            if parent_id == MAIN_BRANCH {
                break;
            }
            current_id = parent_id;
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

        assert_eq!(branch.id(), MAIN_BRANCH);
        assert_eq!(branch.ancestry().len(), 1);
        assert_eq!(branch.ancestry()[0].branch_id, MAIN_BRANCH);
        assert_eq!(branch.ancestry()[0].branch_point_tx, Uuid::max());
    }

    #[test]
    fn open_child_branch() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();

        let child_id = Uuid::now_v7();
        let branch_point = Uuid::now_v7();

        let entry = BranchEntry {
            key: BranchKey { branch_id: child_id },
            value: BranchValue {
                slug: "child".into(),
                meta: serde_json::Map::new(),
                parent_branch_id: MAIN_BRANCH,
                created_from_tx: branch_point,
            },
        };
        let mut batch = storage.db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let branch = Branch::open(storage, child_id).unwrap();
        assert_eq!(branch.id(), child_id);
        assert_eq!(branch.ancestry().len(), 2);
        assert_eq!(branch.ancestry()[0].branch_id, child_id);
        assert_eq!(branch.ancestry()[0].branch_point_tx, Uuid::max());
        assert_eq!(branch.ancestry()[1].branch_id, MAIN_BRANCH);
        assert_eq!(branch.ancestry()[1].branch_point_tx, branch_point);
    }

    #[test]
    fn open_main_by_id() {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path(), test_config()).unwrap();
        let branch = Branch::open(storage, MAIN_BRANCH).unwrap();
        assert_eq!(branch.id(), MAIN_BRANCH);
        assert_eq!(branch.ancestry().len(), 1);
    }
}
