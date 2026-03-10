use uuid::Uuid;

use super::branch_io::{BranchIo, BranchRecord, MAX_BRANCH_DEPTH};
use super::log::LogError;
use super::MAIN_BRANCH;

/// Errors from branch operations.
#[derive(Debug, thiserror::Error)]
pub enum BranchError {
    /// Slug already taken by another branch.
    #[error("branch slug already exists: {0}")]
    SlugExists(String),

    /// Branch not found by slug.
    #[error("branch not found: {0}")]
    SlugNotFound(String),

    /// Branch not found by UUID.
    #[error("branch not found: {0}")]
    NotFound(Uuid),

    /// Parent branch not found.
    #[error("parent branch not found: {0}")]
    ParentNotFound(Uuid),

    /// Branch ancestry exceeds maximum depth.
    #[error("branch depth exceeds maximum of {0}")]
    MaxDepthExceeded(usize),

    /// Storage-level error.
    #[error(transparent)]
    Storage(#[from] LogError),
}

/// Mid-level branch operations.
pub struct BranchStore {
    io: BranchIo,
}

impl BranchStore {
    pub(crate) fn new(io: BranchIo) -> Self {
        Self { io }
    }

    /// Creates a new branch.
    ///
    /// - `parent_id`: parent branch UUID (`MAIN_BRANCH` for main).
    /// - `branch_point_tx`: transaction at which we branch off (`Uuid::max()` = HEAD/latest).
    pub fn create(
        &self,
        slug: &str,
        reasoning: serde_json::Value,
        parent_id: Uuid,
        branch_point_tx: Uuid,
    ) -> Result<BranchRecord, BranchError> {
        if self.io.get_uuid_by_slug(slug)?.is_some() {
            return Err(BranchError::SlugExists(slug.to_string()));
        }

        let id = Uuid::now_v7();

        let ancestry = if parent_id == MAIN_BRANCH {
            vec![(id, Uuid::max()), (MAIN_BRANCH, branch_point_tx)]
        } else {
            let parent = self.io.get(&parent_id)?
                .ok_or(BranchError::ParentNotFound(parent_id))?;
            let mut ancestry = Vec::with_capacity(parent.ancestry.len() + 1);
            ancestry.push((id, Uuid::max()));
            ancestry.push((parent_id, branch_point_tx));
            // Append parent's ancestry, skipping the parent's own self-entry
            for entry in parent.ancestry.iter().skip(1) {
                ancestry.push(*entry);
            }
            if ancestry.len() > MAX_BRANCH_DEPTH {
                return Err(BranchError::MaxDepthExceeded(MAX_BRANCH_DEPTH));
            }
            ancestry
        };

        let created_from_tx = if parent_id == MAIN_BRANCH && branch_point_tx == Uuid::max() {
            Uuid::nil()
        } else {
            branch_point_tx
        };

        let record = BranchRecord {
            id,
            slug: slug.to_string(),
            reasoning,
            created_from_tx,
            ancestry,
        };

        self.io.put_with_index(&record)?;
        Ok(record)
    }

    /// Gets a branch by slug.
    pub fn get_by_slug(&self, slug: &str) -> Result<Option<BranchRecord>, BranchError> {
        let uuid = match self.io.get_uuid_by_slug(slug)? {
            Some(id) => id,
            None => return Ok(None),
        };
        Ok(self.io.get(&uuid)?)
    }

    /// Gets a branch by UUID.
    pub fn get(&self, branch_id: &Uuid) -> Result<Option<BranchRecord>, BranchError> {
        Ok(self.io.get(branch_id)?)
    }

    /// Lists all branches.
    pub fn list_all(&self) -> Result<Vec<BranchRecord>, BranchError> {
        Ok(self.io.scan_all()?)
    }

    /// Returns the precomputed ancestry chain for temporal filtering.
    /// Each entry is `(branch_id, branch_point_tx)`.
    pub fn resolve_ancestry(&self, branch_id: &Uuid) -> Result<Vec<(Uuid, Uuid)>, BranchError> {
        if *branch_id == MAIN_BRANCH {
            return Ok(vec![(MAIN_BRANCH, Uuid::max())]);
        }

        let record = self.io.get(branch_id)?
            .ok_or_else(|| BranchError::NotFound(*branch_id))?;

        Ok(record.ancestry.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assertion::AssertionStore;

    fn setup() -> (AssertionStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = AssertionStore::open(dir.path()).unwrap();
        (store, dir)
    }

    #[test]
    fn create_branch() {
        let (store, _dir) = setup();
        let branches = store.branches();

        let rec = branches
            .create("analysis", serde_json::json!("Deep dive"), MAIN_BRANCH, Uuid::max())
            .unwrap();

        assert_eq!(rec.slug, "analysis");
        assert_eq!(rec.reasoning, serde_json::json!("Deep dive"));
        assert_eq!(rec.created_from_tx, Uuid::nil()); // genesis: main + HEAD
        assert_eq!(rec.ancestry.len(), 2);
        assert_eq!(rec.ancestry[0].0, rec.id);
        assert_eq!(rec.ancestry[0].1, Uuid::max());
        assert_eq!(rec.ancestry[1].0, MAIN_BRANCH);
        assert_eq!(rec.ancestry[1].1, Uuid::max());
    }

    #[test]
    fn duplicate_slug_fails() {
        let (store, _dir) = setup();
        let branches = store.branches();

        branches.create("dup", serde_json::Value::Null, MAIN_BRANCH, Uuid::max()).unwrap();
        let err = branches.create("dup", serde_json::Value::Null, MAIN_BRANCH, Uuid::max()).unwrap_err();
        assert!(matches!(err, BranchError::SlugExists(_)));
    }

    #[test]
    fn get_by_slug() {
        let (store, _dir) = setup();
        let branches = store.branches();

        let created = branches.create("find-me", serde_json::Value::Null, MAIN_BRANCH, Uuid::max()).unwrap();
        let found = branches.get_by_slug("find-me").unwrap().unwrap();
        assert_eq!(found.id, created.id);
        assert!(branches.get_by_slug("ghost").unwrap().is_none());
    }

    #[test]
    fn nested_branch() {
        let (store, _dir) = setup();
        let branches = store.branches();

        let tx_id = Uuid::now_v7();
        let parent = branches.create("parent", serde_json::Value::Null, MAIN_BRANCH, Uuid::max()).unwrap();
        let child = branches.create("child", serde_json::Value::Null, parent.id, tx_id).unwrap();

        let parent_id = child.ancestry.get(1).map(|(id, _)| *id).unwrap_or(MAIN_BRANCH);
        assert_eq!(parent_id, parent.id);
        assert_eq!(child.ancestry.len(), 3);
        assert_eq!(child.ancestry[0].0, child.id);
        assert_eq!(child.ancestry[1].0, parent.id);
        assert_eq!(child.ancestry[1].1, tx_id);
        assert_eq!(child.ancestry[2].0, MAIN_BRANCH);
    }

    #[test]
    fn list_all() {
        let (store, _dir) = setup();
        let branches = store.branches();

        branches.create("a", serde_json::Value::Null, MAIN_BRANCH, Uuid::max()).unwrap();
        branches.create("b", serde_json::Value::Null, MAIN_BRANCH, Uuid::max()).unwrap();

        let all = branches.list_all().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn resolve_ancestry_main() {
        let (store, _dir) = setup();
        let branches = store.branches();

        let ancestry = branches.resolve_ancestry(&MAIN_BRANCH).unwrap();
        assert_eq!(ancestry.len(), 1);
        assert_eq!(ancestry[0].0, MAIN_BRANCH);
        assert_eq!(ancestry[0].1, Uuid::max());
    }

    #[test]
    fn resolve_ancestry_child() {
        let (store, _dir) = setup();
        let branches = store.branches();

        let child = branches.create("child", serde_json::Value::Null, MAIN_BRANCH, Uuid::max()).unwrap();
        let ancestry = branches.resolve_ancestry(&child.id).unwrap();
        assert_eq!(ancestry.len(), 2);
        assert_eq!(ancestry[0].0, child.id);
        assert_eq!(ancestry[0].1, Uuid::max());
        assert_eq!(ancestry[1].0, MAIN_BRANCH);
        assert_eq!(ancestry[1].1, Uuid::max());
    }
}
