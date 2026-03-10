use serde::Serialize;
use uuid::Uuid;

use crate::assertion::{AssertionStore, BranchRecord, BranchError, EntityRecord};

/// Resolved branch detail with full objects instead of UUIDs.
#[derive(Debug)]
pub struct BranchDetail {
    pub id: Uuid,
    pub slug: String,
    pub description: Option<String>,
    pub parent_id: Uuid,
    pub seed_entities: Vec<EntityRecord>,
    pub introduced_entities: Vec<EntityRecord>,
}

/// Branch summary for list output.
#[derive(Debug, Serialize)]
pub struct BranchSummary {
    pub id: Uuid,
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parent_id: Uuid,
    pub seed_count: usize,
    pub introduced_count: usize,
}

/// Errors specific to branch command logic.
#[derive(Debug, thiserror::Error)]
pub enum BranchCommandError {
    #[error("branch not found: {0}")]
    BranchNotFound(String),

    #[error("entity not found: {0}")]
    EntityNotFound(String),

    #[error(transparent)]
    Branch(#[from] BranchError),

    #[error(transparent)]
    Entity(#[from] crate::assertion::EntityError),
}

/// Creates a new branch, resolving entity slugs to UUIDs.
pub fn create_branch(
    input: super::CreateBranchInput,
    store: &AssertionStore,
) -> Result<BranchDetail, BranchCommandError> {
    // Resolve parent
    let parent_id = if input.parent.is_empty() || input.parent == "main" {
        crate::assertion::MAIN_BRANCH
    } else {
        let branches = store.branches();
        let parent = branches.get_by_slug(&input.parent)?
            .ok_or_else(|| BranchCommandError::BranchNotFound(input.parent.clone()))?;
        parent.id
    };

    // Resolve entity slugs
    let entities = store.entities();
    let mut seed_entity_ids = Vec::with_capacity(input.entities.len());
    let mut seed_entities = Vec::with_capacity(input.entities.len());
    for slug in &input.entities {
        let record = entities
            .get_by_slug(slug)?
            .ok_or_else(|| BranchCommandError::EntityNotFound(slug.to_string()))?;
        seed_entity_ids.push(record.id);
        seed_entities.push(record);
    }

    let record = store.branches().create(
        &input.slug,
        input.description.as_deref(),
        parent_id,
        seed_entity_ids,
    )?;

    Ok(BranchDetail {
        id: record.id,
        slug: record.slug,
        description: record.description,
        parent_id: record.parent_id,
        seed_entities,
        introduced_entities: vec![],
    })
}

/// Gets a branch by slug, resolving all UUIDs to full objects.
pub fn get_branch(
    slug: &str,
    store: &AssertionStore,
) -> Result<BranchDetail, BranchCommandError> {
    let record = store
        .branches()
        .get_by_slug(slug)?
        .ok_or_else(|| BranchCommandError::BranchNotFound(slug.to_string()))?;

    resolve_branch(record, store)
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
            description: r.description,
            parent_id: r.parent_id,
            seed_count: r.seed_entities.len(),
            introduced_count: r.introduced_entities.len(),
        })
        .collect())
}

fn resolve_branch(
    record: BranchRecord,
    store: &AssertionStore,
) -> Result<BranchDetail, BranchCommandError> {
    let entities = store.entities();
    let seed_entities: Vec<EntityRecord> = record
        .seed_entities
        .iter()
        .filter_map(|id| entities.get(id).ok().flatten())
        .collect();

    let introduced_entities: Vec<EntityRecord> = record
        .introduced_entities
        .iter()
        .filter_map(|id| entities.get(id).ok().flatten())
        .collect();

    Ok(BranchDetail {
        id: record.id,
        slug: record.slug,
        description: record.description,
        parent_id: record.parent_id,
        seed_entities,
        introduced_entities,
    })
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

        store.entities().create("song-1", None).unwrap();
        store.entities().create("song-2", None).unwrap();

        let detail = create_branch(
            CreateBranchInput {
                slug: "my-branch".into(),
                description: Some("Test branch".into()),
                parent: "main".into(),
                entities: vec!["song-1".into(), "song-2".into()],
            },
            &store,
        )
        .unwrap();

        assert_eq!(detail.slug, "my-branch");
        assert_eq!(detail.seed_entities.len(), 2);
        assert!(detail.introduced_entities.is_empty());

        let fetched = get_branch("my-branch", &store).unwrap();
        assert_eq!(fetched.id, detail.id);
        assert_eq!(fetched.seed_entities.len(), 2);
    }

    #[test]
    fn create_branch_unknown_entity() {
        let (store, _dir) = setup();

        let err = create_branch(
            CreateBranchInput {
                slug: "bad".into(),
                description: None,
                parent: "main".into(),
                entities: vec!["ghost".into()],
            },
            &store,
        )
        .unwrap_err();

        assert!(matches!(err, BranchCommandError::EntityNotFound(_)));
    }

    #[test]
    fn list_branches() {
        let (store, _dir) = setup();

        create_branch(
            CreateBranchInput {
                slug: "a".into(),
                description: None,
                parent: "main".into(),
                entities: vec![],
            },
            &store,
        )
        .unwrap();
        create_branch(
            CreateBranchInput {
                slug: "b".into(),
                description: None,
                parent: "main".into(),
                entities: vec![],
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
