use chrono::Utc;
use uuid::Uuid;

use super::log::LogError;
use super::session_io::{SessionIo, SessionRecord};

/// Errors from session operations.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// Slug already taken by another session.
    #[error("session slug already exists: {0}")]
    SlugExists(String),

    /// Session not found by slug.
    #[error("session not found: {0}")]
    SlugNotFound(String),

    /// Session not found by UUID.
    #[error("session not found: {0}")]
    NotFound(Uuid),

    /// Storage-level error.
    #[error(transparent)]
    Storage(#[from] LogError),
}

/// Mid-level session operations.
pub struct SessionStore {
    io: SessionIo,
}

impl SessionStore {
    pub(crate) fn new(io: SessionIo) -> Self {
        Self { io }
    }

    /// Creates a new session. Entity type and entity UUIDs must be resolved by the caller.
    pub fn create(
        &self,
        slug: &str,
        description: Option<&str>,
        entity_type_ids: Vec<Uuid>,
        seed_entity_ids: Vec<Uuid>,
    ) -> Result<SessionRecord, SessionError> {
        if self.io.get_uuid_by_slug(slug)?.is_some() {
            return Err(SessionError::SlugExists(slug.to_string()));
        }

        let record = SessionRecord {
            id: Uuid::now_v7(),
            slug: slug.to_string(),
            description: description.map(String::from),
            entity_types: entity_type_ids,
            seed_entities: seed_entity_ids,
            introduced_entities: vec![],
            timestamp: Utc::now(),
        };

        self.io.put_with_index(&record)?;
        Ok(record)
    }

    /// Gets a session by slug.
    pub fn get_by_slug(&self, slug: &str) -> Result<Option<SessionRecord>, SessionError> {
        let uuid = match self.io.get_uuid_by_slug(slug)? {
            Some(id) => id,
            None => return Ok(None),
        };
        Ok(self.io.get_latest(&uuid)?)
    }

    /// Gets a session by UUID.
    pub fn get(&self, session_id: &Uuid) -> Result<Option<SessionRecord>, SessionError> {
        Ok(self.io.get_latest(session_id)?)
    }

    /// Lists all sessions.
    pub fn list_all(&self) -> Result<Vec<SessionRecord>, SessionError> {
        Ok(self.io.scan_all_latest()?)
    }

    /// Adds an entity UUID to the session's introduced list.
    pub fn add_introduced(
        &self,
        session_id: &Uuid,
        entity_id: Uuid,
    ) -> Result<SessionRecord, SessionError> {
        let mut record = self
            .io
            .get_latest(session_id)?
            .ok_or_else(|| SessionError::NotFound(*session_id))?;

        record.introduced_entities.push(entity_id);
        record.timestamp = Utc::now();
        self.io.put(&record)?;
        Ok(record)
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
    fn create_session() {
        let (store, _dir) = setup();
        let sessions = store.sessions();

        let et = Uuid::now_v7();
        let e1 = Uuid::now_v7();
        let rec = sessions
            .create("analysis", Some("Deep dive"), vec![et], vec![e1])
            .unwrap();

        assert_eq!(rec.slug, "analysis");
        assert_eq!(rec.description.as_deref(), Some("Deep dive"));
        assert_eq!(rec.entity_types, vec![et]);
        assert_eq!(rec.seed_entities, vec![e1]);
        assert!(rec.introduced_entities.is_empty());
    }

    #[test]
    fn duplicate_slug_fails() {
        let (store, _dir) = setup();
        let sessions = store.sessions();

        sessions.create("dup", None, vec![], vec![]).unwrap();
        let err = sessions.create("dup", None, vec![], vec![]).unwrap_err();
        assert!(matches!(err, SessionError::SlugExists(_)));
    }

    #[test]
    fn get_by_slug() {
        let (store, _dir) = setup();
        let sessions = store.sessions();

        let created = sessions.create("find-me", None, vec![], vec![]).unwrap();
        let found = sessions.get_by_slug("find-me").unwrap().unwrap();
        assert_eq!(found.id, created.id);
        assert!(sessions.get_by_slug("ghost").unwrap().is_none());
    }

    #[test]
    fn add_introduced() {
        let (store, _dir) = setup();
        let sessions = store.sessions();

        let created = sessions.create("growing", None, vec![], vec![]).unwrap();
        let new_entity = Uuid::now_v7();

        let updated = sessions.add_introduced(&created.id, new_entity).unwrap();
        assert_eq!(updated.introduced_entities, vec![new_entity]);

        let another = Uuid::now_v7();
        let updated2 = sessions.add_introduced(&created.id, another).unwrap();
        assert_eq!(updated2.introduced_entities, vec![new_entity, another]);
    }

    #[test]
    fn list_all() {
        let (store, _dir) = setup();
        let sessions = store.sessions();

        sessions.create("a", None, vec![], vec![]).unwrap();
        sessions.create("b", None, vec![], vec![]).unwrap();

        let all = sessions.list_all().unwrap();
        assert_eq!(all.len(), 2);
    }
}
