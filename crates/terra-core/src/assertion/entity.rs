use uuid::Uuid;

use super::entity_io::{EntityIo, EntityRecord, EntityStatus};
use super::log::LogError;

/// Errors from entity operations.
#[derive(Debug, thiserror::Error)]
pub enum EntityError {
    /// Slug already taken by another entity.
    #[error("slug already exists: {0}")]
    SlugExists(String),

    /// Entity not found by UUID.
    #[error("entity not found: {0}")]
    NotFound(Uuid),

    /// Entity not found by slug.
    #[error("entity not found by slug: {0}")]
    SlugNotFound(String),

    /// Entity is already in the requested status.
    #[error("entity {0} is already {1}")]
    AlreadyInStatus(Uuid, &'static str),

    /// Storage-level error.
    #[error(transparent)]
    Storage(#[from] LogError),
}

/// Mid-level entity operations: create, delete, restore, find.
pub struct EntityStore {
    io: EntityIo,
}

impl EntityStore {
    pub(crate) fn new(io: EntityIo) -> Self {
        Self { io }
    }

    /// Creates a new entity. Generates UUID, writes active record + slug index.
    pub fn create(
        &self,
        slug: &str,
        description: Option<&str>,
    ) -> Result<EntityRecord, EntityError> {
        if self.io.get_uuid_by_slug(slug)?.is_some() {
            return Err(EntityError::SlugExists(slug.to_string()));
        }

        let id = Uuid::now_v7();
        let record = EntityRecord {
            id,
            slug: slug.to_string(),
            status: EntityStatus::Active,
            description: description.map(String::from),
            tx_id: Uuid::now_v7(),
        };

        self.io.put_with_index(&record)?;
        Ok(record)
    }

    /// Marks an entity as deleted.
    pub fn delete(&self, entity_id: &Uuid) -> Result<EntityRecord, EntityError> {
        let current = self.require_entity(entity_id)?;

        if current.status == EntityStatus::Deleted {
            return Err(EntityError::AlreadyInStatus(*entity_id, "deleted"));
        }

        let record = EntityRecord {
            id: current.id,
            slug: current.slug,
            status: EntityStatus::Deleted,
            description: current.description,
            tx_id: Uuid::now_v7(),
        };

        self.io.put(&record)?;
        Ok(record)
    }

    /// Restores a deleted entity.
    pub fn restore(&self, entity_id: &Uuid) -> Result<EntityRecord, EntityError> {
        let current = self.require_entity(entity_id)?;

        if current.status == EntityStatus::Active {
            return Err(EntityError::AlreadyInStatus(*entity_id, "active"));
        }

        let record = EntityRecord {
            id: current.id,
            slug: current.slug,
            status: EntityStatus::Active,
            description: current.description,
            tx_id: Uuid::now_v7(),
        };

        self.io.put(&record)?;
        Ok(record)
    }

    /// Gets the current state of an entity by UUID.
    pub fn get(&self, entity_id: &Uuid) -> Result<Option<EntityRecord>, EntityError> {
        Ok(self.io.get_latest(entity_id)?)
    }

    /// Gets the current state of an entity by slug.
    pub fn get_by_slug(&self, slug: &str) -> Result<Option<EntityRecord>, EntityError> {
        let uuid = match self.io.get_uuid_by_slug(slug)? {
            Some(id) => id,
            None => return Ok(None),
        };
        Ok(self.io.get_latest(&uuid)?)
    }

    /// Returns true if entity exists (regardless of status).
    pub fn exists(&self, entity_id: &Uuid) -> Result<bool, EntityError> {
        Ok(self.io.get_latest(entity_id)?.is_some())
    }

    /// Returns true if entity exists and is currently active.
    pub fn is_active(&self, entity_id: &Uuid) -> Result<bool, EntityError> {
        Ok(self.io.get_latest(entity_id)?
            .map(|r| r.status == EntityStatus::Active)
            .unwrap_or(false))
    }

    /// Lists all entities with their current state.
    pub fn list_all(&self) -> Result<Vec<EntityRecord>, EntityError> {
        Ok(self.io.scan_all_latest()?)
    }

    /// Lists only currently active entities.
    pub fn list_active(&self) -> Result<Vec<EntityRecord>, EntityError> {
        let all = self.io.scan_all_latest()?;
        Ok(all.into_iter().filter(|r| r.status == EntityStatus::Active).collect())
    }

    /// Returns full history of an entity.
    pub fn history(&self, entity_id: &Uuid) -> Result<Vec<EntityRecord>, EntityError> {
        let records = self.io.get_history(entity_id)?;
        if records.is_empty() {
            return Err(EntityError::NotFound(*entity_id));
        }
        Ok(records)
    }

    fn require_entity(&self, entity_id: &Uuid) -> Result<EntityRecord, EntityError> {
        self.io.get_latest(entity_id)?
            .ok_or_else(|| EntityError::NotFound(*entity_id))
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
    fn create_entity() {
        let (store, _dir) = setup();
        let entities = store.entities();

        let rec = entities.create("alpha", Some("First entity")).unwrap();
        assert_eq!(rec.slug, "alpha");
        assert_eq!(rec.status, EntityStatus::Active);
        assert_eq!(rec.description.as_deref(), Some("First entity"));
    }

    #[test]
    fn create_duplicate_slug_fails() {
        let (store, _dir) = setup();
        let entities = store.entities();

        entities.create("alpha", None).unwrap();
        let err = entities.create("alpha", None).unwrap_err();
        assert!(matches!(err, EntityError::SlugExists(_)));
    }

    #[test]
    fn get_by_uuid_and_slug() {
        let (store, _dir) = setup();
        let entities = store.entities();

        let created = entities.create("bravo", None).unwrap();

        let by_id = entities.get(&created.id).unwrap().unwrap();
        assert_eq!(by_id.slug, "bravo");

        let by_slug = entities.get_by_slug("bravo").unwrap().unwrap();
        assert_eq!(by_slug.id, created.id);

        assert!(entities.get_by_slug("nonexistent").unwrap().is_none());
    }

    #[test]
    fn delete_and_restore() {
        let (store, _dir) = setup();
        let entities = store.entities();

        let created = entities.create("charlie", None).unwrap();
        assert!(entities.is_active(&created.id).unwrap());

        let deleted = entities.delete(&created.id).unwrap();
        assert_eq!(deleted.status, EntityStatus::Deleted);
        assert!(!entities.is_active(&created.id).unwrap());
        assert!(entities.exists(&created.id).unwrap());

        let restored = entities.restore(&created.id).unwrap();
        assert_eq!(restored.status, EntityStatus::Active);
        assert!(entities.is_active(&created.id).unwrap());
    }

    #[test]
    fn delete_already_deleted_fails() {
        let (store, _dir) = setup();
        let entities = store.entities();

        let created = entities.create("delta", None).unwrap();
        entities.delete(&created.id).unwrap();

        let err = entities.delete(&created.id).unwrap_err();
        assert!(matches!(err, EntityError::AlreadyInStatus(_, "deleted")));
    }

    #[test]
    fn restore_already_active_fails() {
        let (store, _dir) = setup();
        let entities = store.entities();

        let created = entities.create("echo", None).unwrap();
        let err = entities.restore(&created.id).unwrap_err();
        assert!(matches!(err, EntityError::AlreadyInStatus(_, "active")));
    }

    #[test]
    fn delete_nonexistent_fails() {
        let (store, _dir) = setup();
        let entities = store.entities();

        let err = entities.delete(&Uuid::now_v7()).unwrap_err();
        assert!(matches!(err, EntityError::NotFound(_)));
    }

    #[test]
    fn list_active_excludes_deleted() {
        let (store, _dir) = setup();
        let entities = store.entities();

        let a = entities.create("one", None).unwrap();
        entities.create("two", None).unwrap();
        entities.delete(&a.id).unwrap();

        let active = entities.list_active().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].slug, "two");

        let all = entities.list_all().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn history_tracks_all_changes() {
        let (store, _dir) = setup();
        let entities = store.entities();

        let created = entities.create("foxtrot", None).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1));
        entities.delete(&created.id).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1));
        entities.restore(&created.id).unwrap();

        let history = entities.history(&created.id).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].status, EntityStatus::Active);
        assert_eq!(history[1].status, EntityStatus::Deleted);
        assert_eq!(history[2].status, EntityStatus::Active);
    }
}
