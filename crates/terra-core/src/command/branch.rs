use serde::Serialize;
use uuid::Uuid;

use crate::assertion::{AssertionStore, BranchError, MAIN_BRANCH};

/// Resolved branch detail.
#[derive(Debug)]
pub struct BranchDetail {
    pub id: Uuid,
    pub slug: String,
    pub reasoning: serde_json::Value,
    pub created_from_tx: Uuid,
    /// Derived from `ancestry[1].0` or `MAIN_BRANCH`.
    pub parent_id: Uuid,
}

/// Branch summary for list output.
#[derive(Debug, Serialize)]
pub struct BranchSummary {
    pub id: Uuid,
    pub slug: String,
    pub reasoning: serde_json::Value,
    /// Derived from `ancestry[1].0` or `MAIN_BRANCH`.
    pub parent_id: Uuid,
}

/// Errors specific to branch command logic.
#[derive(Debug, thiserror::Error)]
pub enum BranchCommandError {
    #[error("branch not found: {0}")]
    BranchNotFound(String),

    #[error(transparent)]
    Branch(#[from] BranchError),
}

/// Creates a new branch.
pub fn create_branch(
    input: super::CreateBranchInput,
    store: &AssertionStore,
) -> Result<BranchDetail, BranchCommandError> {
    let parent_id = if input.parent.is_empty() || input.parent == "main" {
        MAIN_BRANCH
    } else {
        let branches = store.branches();
        let parent = branches.get_by_slug(&input.parent)?
            .ok_or_else(|| BranchCommandError::BranchNotFound(input.parent.clone()))?;
        parent.id
    };

    let branch_point_tx = input.from_tx.unwrap_or(Uuid::max());

    let record = store.branches().create(
        &input.slug,
        input.reasoning,
        parent_id,
        branch_point_tx,
    )?;

    Ok(BranchDetail {
        id: record.id,
        slug: record.slug,
        reasoning: record.reasoning,
        created_from_tx: record.created_from_tx,
        parent_id: record.ancestry.get(1).map(|(id, _)| *id).unwrap_or(MAIN_BRANCH),
    })
}

/// Gets a branch by slug.
pub fn get_branch(
    slug: &str,
    store: &AssertionStore,
) -> Result<BranchDetail, BranchCommandError> {
    let record = store
        .branches()
        .get_by_slug(slug)?
        .ok_or_else(|| BranchCommandError::BranchNotFound(slug.to_string()))?;

    Ok(BranchDetail {
        id: record.id,
        slug: record.slug,
        reasoning: record.reasoning,
        created_from_tx: record.created_from_tx,
        parent_id: record.ancestry.get(1).map(|(id, _)| *id).unwrap_or(MAIN_BRANCH),
    })
}

/// Lists all branches as summaries.
pub fn list_branches(
    store: &AssertionStore,
) -> Result<Vec<BranchSummary>, BranchCommandError> {
    let records = store.branches().list_all()?;
    Ok(records
        .into_iter()
        .map(|r| BranchSummary {
            id: r.id,
            slug: r.slug,
            reasoning: r.reasoning,
            parent_id: r.ancestry.get(1).map(|(id, _)| *id).unwrap_or(MAIN_BRANCH),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::CreateBranchInput;

    fn setup() -> (AssertionStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = AssertionStore::open(dir.path()).unwrap();
        (store, dir)
    }

    #[test]
    fn create_and_get_branch() {
        let (store, _dir) = setup();

        let detail = create_branch(
            CreateBranchInput {
                slug: "my-branch".into(),
                reasoning: serde_json::json!("Test branch"),
                parent: "main".into(),
                from_tx: None,
            },
            &store,
        )
        .unwrap();

        assert_eq!(detail.slug, "my-branch");
        assert_eq!(detail.reasoning, serde_json::json!("Test branch"));
        assert_eq!(detail.parent_id, MAIN_BRANCH);

        let fetched = get_branch("my-branch", &store).unwrap();
        assert_eq!(fetched.id, detail.id);
    }

    #[test]
    fn list_branches() {
        let (store, _dir) = setup();

        create_branch(
            CreateBranchInput {
                slug: "a".into(),
                reasoning: serde_json::Value::Null,
                parent: "main".into(),
                from_tx: None,
            },
            &store,
        )
        .unwrap();
        create_branch(
            CreateBranchInput {
                slug: "b".into(),
                reasoning: serde_json::Value::Null,
                parent: "main".into(),
                from_tx: None,
            },
            &store,
        )
        .unwrap();

        let list = super::list_branches(&store).unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn branch_not_found() {
        let (store, _dir) = setup();

        let err = get_branch("nope", &store).unwrap_err();
        assert!(matches!(err, BranchCommandError::BranchNotFound(_)));
    }
}
