use chrono::Utc;
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

    /// Creates a new branch. Resolves parent ancestry and computes branch_point_us.
    pub fn create(
        &self,
        slug: &str,
        description: Option<&str>,
        parent_id: Uuid,
        seed_entity_ids: Vec<Uuid>,
    ) -> Result<BranchRecord, BranchError> {
        if self.io.get_uuid_by_slug(slug)?.is_some() {
            return Err(BranchError::SlugExists(slug.to_string()));
        }

        let now = Utc::now();
        let branch_point_us = now.timestamp_micros();
        let id = Uuid::now_v7();

        let ancestry = if parent_id == MAIN_BRANCH {
            vec![id, MAIN_BRANCH]
        } else {
            let parent = self.io.get(&parent_id)?
                .ok_or(BranchError::ParentNotFound(parent_id))?;
            let mut ancestry = Vec::with_capacity(parent.ancestry.len() + 1);
            ancestry.push(id);
            ancestry.extend_from_slice(&parent.ancestry);
            if ancestry.len() > MAX_BRANCH_DEPTH {
                return Err(BranchError::MaxDepthExceeded(MAX_BRANCH_DEPTH));
            }
            ancestry
        };

        let record = BranchRecord {
            id,
            slug: slug.to_string(),
            description: description.map(String::from),
            parent_id,
            branch_point_us,
            ancestry,
            seed_entities: seed_entity_ids,
            introduced_entities: vec![],
            timestamp: now,
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

    /// Adds an entity UUID to the branch's introduced list.
    pub fn add_introduced(
        &self,
        branch_id: &Uuid,
        entity_id: Uuid,
    ) -> Result<BranchRecord, BranchError> {
        let mut record = self.io.get(branch_id)?
            .ok_or_else(|| BranchError::NotFound(*branch_id))?;

        record.introduced_entities.push(entity_id);
        record.timestamp = Utc::now();
        self.io.put(&record)?;
        Ok(record)
    }

    /// Builds the ancestry chain with temporal filtering info.
    /// Returns `Vec<(branch_id, branch_point_us)>` — for main branch, branch_point_us is i64::MAX.
    pub fn resolve_ancestry(&self, branch_id: &Uuid) -> Result<Vec<(Uuid, i64)>, BranchError> {
        if *branch_id == MAIN_BRANCH {
            return Ok(vec![(MAIN_BRANCH, i64::MAX)]);
        }

        let record = self.io.get(branch_id)?
            .ok_or_else(|| BranchError::NotFound(*branch_id))?;

        let mut result = Vec::with_capacity(record.ancestry.len());

        // First entry is self — no temporal filter (i64::MAX = see everything)
        result.push((record.id, i64::MAX));

        // Walk ancestry: for each parent, we need THEIR branch_point_us
        for (i, ancestor_id) in record.ancestry.iter().enumerate().skip(1) {
            if *ancestor_id == MAIN_BRANCH {
                // Main branch is filtered by the child's branch_point_us
                let child_branch_point = if i == 1 {
                    record.branch_point_us
                } else {
                    // Get the branch_point_us of the child that branched from this ancestor
                    let child_id = record.ancestry[i - 1];
                    let child_record = self.io.get(&child_id)?
                        .ok_or_else(|| BranchError::NotFound(child_id))?;
                    child_record.branch_point_us
                };
                result.push((MAIN_BRANCH, child_branch_point));
            } else {
                let ancestor = self.io.get(ancestor_id)?
                    .ok_or_else(|| BranchError::NotFound(*ancestor_id))?;
                // Parent is filtered by its child's branch_point_us
                let child_branch_point = if i == 1 {
                    record.branch_point_us
                } else {
                    let child_id = record.ancestry[i - 1];
                    let child_record = self.io.get(&child_id)?
                        .ok_or_else(|| BranchError::NotFound(child_id))?;
                    child_record.branch_point_us
                };
                result.push((ancestor.id, child_branch_point));
            }
        }

        Ok(result)
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
            .create("analysis", Some("Deep dive"), MAIN_BRANCH, vec![])
            .unwrap();

        assert_eq!(rec.slug, "analysis");
        assert_eq!(rec.description.as_deref(), Some("Deep dive"));
        assert_eq!(rec.parent_id, MAIN_BRANCH);
        assert_eq!(rec.ancestry.len(), 2);
        assert_eq!(rec.ancestry[0], rec.id);
        assert_eq!(rec.ancestry[1], MAIN_BRANCH);
    }

    #[test]
    fn duplicate_slug_fails() {
        let (store, _dir) = setup();
        let branches = store.branches();

        branches.create("dup", None, MAIN_BRANCH, vec![]).unwrap();
        let err = branches.create("dup", None, MAIN_BRANCH, vec![]).unwrap_err();
        assert!(matches!(err, BranchError::SlugExists(_)));
    }

    #[test]
    fn get_by_slug() {
        let (store, _dir) = setup();
        let branches = store.branches();

        let created = branches.create("find-me", None, MAIN_BRANCH, vec![]).unwrap();
        let found = branches.get_by_slug("find-me").unwrap().unwrap();
        assert_eq!(found.id, created.id);
        assert!(branches.get_by_slug("ghost").unwrap().is_none());
    }

    #[test]
    fn nested_branch() {
        let (store, _dir) = setup();
        let branches = store.branches();

        let parent = branches.create("parent", None, MAIN_BRANCH, vec![]).unwrap();
        let child = branches.create("child", None, parent.id, vec![]).unwrap();

        assert_eq!(child.parent_id, parent.id);
        assert_eq!(child.ancestry.len(), 3);
        assert_eq!(child.ancestry[0], child.id);
        assert_eq!(child.ancestry[1], parent.id);
        assert_eq!(child.ancestry[2], MAIN_BRANCH);
    }

    #[test]
    fn add_introduced() {
        let (store, _dir) = setup();
        let branches = store.branches();

        let created = branches.create("growing", None, MAIN_BRANCH, vec![]).unwrap();
        let new_entity = Uuid::now_v7();

        let updated = branches.add_introduced(&created.id, new_entity).unwrap();
        assert_eq!(updated.introduced_entities, vec![new_entity]);
    }

    #[test]
    fn list_all() {
        let (store, _dir) = setup();
        let branches = store.branches();

        branches.create("a", None, MAIN_BRANCH, vec![]).unwrap();
        branches.create("b", None, MAIN_BRANCH, vec![]).unwrap();

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
        assert_eq!(ancestry[0].1, i64::MAX);
    }

    #[test]
    fn resolve_ancestry_child() {
        let (store, _dir) = setup();
        let branches = store.branches();

        let child = branches.create("child", None, MAIN_BRANCH, vec![]).unwrap();
        let ancestry = branches.resolve_ancestry(&child.id).unwrap();
        assert_eq!(ancestry.len(), 2);
        assert_eq!(ancestry[0].0, child.id);
        assert_eq!(ancestry[0].1, i64::MAX);
        assert_eq!(ancestry[1].0, MAIN_BRANCH);
        assert_eq!(ancestry[1].1, child.branch_point_us);
    }
}
