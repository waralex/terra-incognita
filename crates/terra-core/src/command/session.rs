use uuid::Uuid;

use crate::assertion::{AssertionStore, EntityRecord, SessionError, SessionRecord};
use crate::schema::{EntityType, SchemaRegistry};

/// Resolved session detail with full objects instead of UUIDs.
#[derive(Debug)]
pub struct SessionDetail {
    pub id: Uuid,
    pub slug: String,
    pub description: Option<String>,
    pub entity_types: Vec<EntityType>,
    pub seed_entities: Vec<EntityRecord>,
    pub introduced_entities: Vec<EntityRecord>,
}

/// Session summary for list output.
#[derive(Debug)]
pub struct SessionSummary {
    pub id: Uuid,
    pub slug: String,
    pub description: Option<String>,
    pub entity_type_count: usize,
    pub seed_count: usize,
    pub introduced_count: usize,
}

/// Errors specific to session command logic.
#[derive(Debug, thiserror::Error)]
pub enum SessionCommandError {
    #[error("session not found: {0}")]
    SessionNotFound(String),

    #[error("entity type not found: {0}")]
    EntityTypeNotFound(String),

    #[error("entity not found: {0}")]
    EntityNotFound(String),

    #[error(transparent)]
    Session(#[from] SessionError),

    #[error(transparent)]
    Schema(#[from] crate::schema::SchemaError),

    #[error(transparent)]
    Entity(#[from] crate::assertion::EntityError),
}

/// Creates a new session, resolving entity type and entity slugs to UUIDs.
pub fn create_session(
    input: super::CreateSessionInput,
    registry: &SchemaRegistry,
    store: &AssertionStore,
) -> Result<SessionDetail, SessionCommandError> {
    // Resolve entity type slugs
    let mut entity_type_ids = Vec::with_capacity(input.entity_types.len());
    let mut entity_types = Vec::with_capacity(input.entity_types.len());
    for slug in &input.entity_types {
        let et = registry.get_entity_type(slug).map_err(|_| {
            SessionCommandError::EntityTypeNotFound(slug.to_string())
        })?;
        entity_type_ids.push(et.id);
        entity_types.push(et);
    }

    // Resolve entity slugs
    let entities = store.entities();
    let mut seed_entity_ids = Vec::with_capacity(input.entities.len());
    let mut seed_entities = Vec::with_capacity(input.entities.len());
    for slug in &input.entities {
        let record = entities
            .get_by_slug(slug)?
            .ok_or_else(|| SessionCommandError::EntityNotFound(slug.to_string()))?;
        seed_entity_ids.push(record.id);
        seed_entities.push(record);
    }

    // Create session
    let record = store.sessions().create(
        &input.slug,
        input.description.as_deref(),
        entity_type_ids,
        seed_entity_ids,
    )?;

    Ok(SessionDetail {
        id: record.id,
        slug: record.slug,
        description: record.description,
        entity_types,
        seed_entities,
        introduced_entities: vec![],
    })
}

/// Gets a session by slug, resolving all UUIDs to full objects.
pub fn get_session(
    slug: &str,
    registry: &SchemaRegistry,
    store: &AssertionStore,
) -> Result<SessionDetail, SessionCommandError> {
    let record = store
        .sessions()
        .get_by_slug(slug)?
        .ok_or_else(|| SessionCommandError::SessionNotFound(slug.to_string()))?;

    resolve_session(record, registry, store)
}

/// Lists all sessions as summaries.
pub fn list_sessions(
    store: &AssertionStore,
) -> Result<Vec<SessionSummary>, SessionCommandError> {
    let records = store.sessions().list_all()?;
    Ok(records
        .into_iter()
        .map(|r| SessionSummary {
            id: r.id,
            slug: r.slug,
            description: r.description,
            entity_type_count: r.entity_types.len(),
            seed_count: r.seed_entities.len(),
            introduced_count: r.introduced_entities.len(),
        })
        .collect())
}

fn resolve_session(
    record: SessionRecord,
    registry: &SchemaRegistry,
    store: &AssertionStore,
) -> Result<SessionDetail, SessionCommandError> {
    let entity_types: Vec<EntityType> = record
        .entity_types
        .iter()
        .filter_map(|id| registry.get_entity_type_by_id(id).ok())
        .collect();

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

    Ok(SessionDetail {
        id: record.id,
        slug: record.slug,
        description: record.description,
        entity_types,
        seed_entities,
        introduced_entities,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{execute, Command, CommandResult, CreateEntityType, CreateSessionInput};

    fn setup() -> (SchemaRegistry, AssertionStore, tempfile::TempDir) {
        let registry = SchemaRegistry::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let store = AssertionStore::open(dir.path()).unwrap();
        (registry, store, dir)
    }

    #[test]
    fn create_and_get_session() {
        let (mut reg, store, _dir) = setup();

        execute(
            Command::CreateEntityTypes(vec![
                CreateEntityType { slug: "track".into(), description: None, properties: vec![] },
                CreateEntityType { slug: "album".into(), description: None, properties: vec![] },
            ]),
            &mut reg,
            &store,
        )
        .unwrap();

        store.entities().create("song-1", None).unwrap();
        store.entities().create("song-2", None).unwrap();

        let detail = create_session(
            CreateSessionInput {
                slug: "my-session".into(),
                description: Some("Test session".into()),
                entity_types: vec!["track".into(), "album".into()],
                entities: vec!["song-1".into(), "song-2".into()],
            },
            &reg,
            &store,
        )
        .unwrap();

        assert_eq!(detail.slug, "my-session");
        assert_eq!(detail.entity_types.len(), 2);
        assert_eq!(detail.seed_entities.len(), 2);
        assert!(detail.introduced_entities.is_empty());

        // Get it back
        let fetched = get_session("my-session", &reg, &store).unwrap();
        assert_eq!(fetched.id, detail.id);
        assert_eq!(fetched.entity_types.len(), 2);
        assert_eq!(fetched.seed_entities.len(), 2);
    }

    #[test]
    fn create_session_unknown_entity_type() {
        let (reg, store, _dir) = setup();

        let err = create_session(
            CreateSessionInput {
                slug: "bad".into(),
                description: None,
                entity_types: vec!["nonexistent".into()],
                entities: vec![],
            },
            &reg,
            &store,
        )
        .unwrap_err();

        assert!(matches!(err, SessionCommandError::EntityTypeNotFound(_)));
    }

    #[test]
    fn create_session_unknown_entity() {
        let (mut reg, store, _dir) = setup();

        execute(
            Command::CreateEntityTypes(vec![CreateEntityType {
                slug: "track".into(),
                description: None,
                properties: vec![],
            }]),
            &mut reg,
            &store,
        )
        .unwrap();

        let err = create_session(
            CreateSessionInput {
                slug: "bad".into(),
                description: None,
                entity_types: vec!["track".into()],
                entities: vec!["ghost".into()],
            },
            &reg,
            &store,
        )
        .unwrap_err();

        assert!(matches!(err, SessionCommandError::EntityNotFound(_)));
    }

    #[test]
    fn list_sessions() {
        let (reg, store, _dir) = setup();

        create_session(
            CreateSessionInput {
                slug: "a".into(),
                description: None,
                entity_types: vec![],
                entities: vec![],
            },
            &reg,
            &store,
        )
        .unwrap();
        create_session(
            CreateSessionInput {
                slug: "b".into(),
                description: None,
                entity_types: vec![],
                entities: vec![],
            },
            &reg,
            &store,
        )
        .unwrap();

        let list = super::list_sessions(&store).unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn session_not_found() {
        let (reg, store, _dir) = setup();

        let err = get_session("nope", &reg, &store).unwrap_err();
        assert!(matches!(err, SessionCommandError::SessionNotFound(_)));
    }
}
