use uuid::Uuid;

use super::investigation_io::{InvestigationIo, InvestigationRecord, InvestigationStatus};
use super::log::LogError;

/// Errors from investigation operations.
#[derive(Debug, thiserror::Error)]
pub enum InvestigationError {
    /// Slug already taken by another investigation.
    #[error("investigation slug already exists: {0}")]
    SlugExists(String),

    /// Investigation not found by UUID.
    #[error("investigation not found: {0}")]
    NotFound(Uuid),

    /// Investigation not found by slug.
    #[error("investigation not found by slug: {0}")]
    SlugNotFound(String),

    /// Investigation is already in the requested status.
    #[error("investigation {0} is already {1}")]
    AlreadyInStatus(Uuid, &'static str),

    /// Storage-level error.
    #[error(transparent)]
    Storage(#[from] LogError),
}

/// Mid-level investigation operations: create, update notes, close, list.
///
/// Branch-aware: writes go to the configured branch, reads walk the ancestry chain.
pub struct InvestigationStore {
    io: InvestigationIo,
}

impl InvestigationStore {
    pub(crate) fn new(io: InvestigationIo) -> Self {
        Self { io }
    }

    /// Creates a new investigation. Generates UUID, writes record + slug index.
    pub fn create(
        &self,
        slug: &str,
        goal: serde_json::Value,
        reasoning: &str,
        context: serde_json::Value,
        tx_id: Uuid,
    ) -> Result<InvestigationRecord, InvestigationError> {
        if self.io.get_uuid_by_slug(slug)?.is_some() {
            return Err(InvestigationError::SlugExists(slug.to_string()));
        }

        let record = InvestigationRecord {
            id: Uuid::now_v7(),
            slug: slug.to_string(),
            status: InvestigationStatus::Open,
            goal,
            reasoning: reasoning.to_string(),
            context,
            notes: serde_json::Value::Null,
            resolution: None,
            tx_id,
        };

        self.io.put_with_index(&record)?;
        Ok(record)
    }

    /// Updates the notes on an open investigation.
    pub fn update_notes(
        &self,
        investigation_id: &Uuid,
        notes: serde_json::Value,
        tx_id: Uuid,
    ) -> Result<InvestigationRecord, InvestigationError> {
        let current = self.require(investigation_id)?;

        if current.status == InvestigationStatus::Closed {
            return Err(InvestigationError::AlreadyInStatus(*investigation_id, "closed"));
        }

        let record = InvestigationRecord {
            notes,
            tx_id,
            ..current
        };

        self.io.put(&record)?;
        Ok(record)
    }

    /// Closes an investigation with a resolution.
    pub fn close(
        &self,
        investigation_id: &Uuid,
        resolution: serde_json::Value,
        tx_id: Uuid,
    ) -> Result<InvestigationRecord, InvestigationError> {
        let current = self.require(investigation_id)?;

        if current.status == InvestigationStatus::Closed {
            return Err(InvestigationError::AlreadyInStatus(*investigation_id, "closed"));
        }

        let record = InvestigationRecord {
            status: InvestigationStatus::Closed,
            resolution: Some(resolution),
            tx_id,
            ..current
        };

        self.io.put(&record)?;
        Ok(record)
    }

    /// Gets the current state of an investigation by UUID.
    pub fn get(&self, investigation_id: &Uuid) -> Result<Option<InvestigationRecord>, InvestigationError> {
        Ok(self.io.get_latest(investigation_id)?)
    }

    /// Gets the current state of an investigation by slug.
    pub fn get_by_slug(&self, slug: &str) -> Result<Option<InvestigationRecord>, InvestigationError> {
        let uuid = match self.io.get_uuid_by_slug(slug)? {
            Some(id) => id,
            None => return Ok(None),
        };
        Ok(self.io.get_latest(&uuid)?)
    }

    /// Lists all investigations with their current state, bounded by tx_id.
    pub fn list_all_at(&self, upper_bound: Uuid) -> Result<Vec<InvestigationRecord>, InvestigationError> {
        Ok(self.io.scan_all_latest_at(upper_bound)?)
    }

    /// Lists only currently open investigations.
    pub fn list_open_at(&self, upper_bound: Uuid) -> Result<Vec<InvestigationRecord>, InvestigationError> {
        let all = self.io.scan_all_latest_at(upper_bound)?;
        Ok(all.into_iter().filter(|r| r.status == InvestigationStatus::Open).collect())
    }

    fn require(&self, investigation_id: &Uuid) -> Result<InvestigationRecord, InvestigationError> {
        self.io.get_latest(investigation_id)?
            .ok_or_else(|| InvestigationError::NotFound(*investigation_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assertion::{AssertionStore, MAIN_BRANCH};

    fn setup() -> (AssertionStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = AssertionStore::open(dir.path()).unwrap();
        (store, dir)
    }

    #[test]
    fn create_investigation() {
        let (store, _dir) = setup();
        let investigations = store.investigations(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);

        let rec = investigations.create(
            "explore-orders",
            serde_json::json!("Understand the orders table"),
            "Starting exploration",
            serde_json::json!({}),
            Uuid::now_v7(),
        ).unwrap();

        assert_eq!(rec.slug, "explore-orders");
        assert_eq!(rec.status, InvestigationStatus::Open);
        assert!(rec.resolution.is_none());
    }

    #[test]
    fn duplicate_slug_fails() {
        let (store, _dir) = setup();
        let investigations = store.investigations(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);

        investigations.create("alpha", serde_json::json!("goal"), "r", serde_json::json!({}), Uuid::now_v7()).unwrap();
        let err = investigations.create("alpha", serde_json::json!("goal2"), "r", serde_json::json!({}), Uuid::now_v7()).unwrap_err();
        assert!(matches!(err, InvestigationError::SlugExists(_)));
    }

    #[test]
    fn update_notes() {
        let (store, _dir) = setup();
        let investigations = store.investigations(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);

        let rec = investigations.create("inv1", serde_json::json!("goal"), "r", serde_json::json!({}), Uuid::now_v7()).unwrap();

        let updated = investigations.update_notes(
            &rec.id,
            serde_json::json!({"finding": "interesting pattern"}),
            Uuid::now_v7(),
        ).unwrap();

        assert_eq!(updated.notes, serde_json::json!({"finding": "interesting pattern"}));
        assert_eq!(updated.status, InvestigationStatus::Open);
    }

    #[test]
    fn close_investigation() {
        let (store, _dir) = setup();
        let investigations = store.investigations(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);

        let rec = investigations.create("inv2", serde_json::json!("goal"), "r", serde_json::json!({}), Uuid::now_v7()).unwrap();

        let closed = investigations.close(
            &rec.id,
            serde_json::json!({"conclusion": "done"}),
            Uuid::now_v7(),
        ).unwrap();

        assert_eq!(closed.status, InvestigationStatus::Closed);
        assert_eq!(closed.resolution.unwrap(), serde_json::json!({"conclusion": "done"}));
    }

    #[test]
    fn close_already_closed_fails() {
        let (store, _dir) = setup();
        let investigations = store.investigations(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);

        let rec = investigations.create("inv3", serde_json::json!("goal"), "r", serde_json::json!({}), Uuid::now_v7()).unwrap();
        investigations.close(&rec.id, serde_json::json!("done"), Uuid::now_v7()).unwrap();

        let err = investigations.close(&rec.id, serde_json::json!("again"), Uuid::now_v7()).unwrap_err();
        assert!(matches!(err, InvestigationError::AlreadyInStatus(_, "closed")));
    }

    #[test]
    fn list_open_excludes_closed() {
        let (store, _dir) = setup();
        let investigations = store.investigations(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);

        let a = investigations.create("inv-a", serde_json::json!("goal"), "r", serde_json::json!({}), Uuid::now_v7()).unwrap();
        investigations.create("inv-b", serde_json::json!("goal"), "r", serde_json::json!({}), Uuid::now_v7()).unwrap();
        investigations.close(&a.id, serde_json::json!("done"), Uuid::now_v7()).unwrap();

        let open = investigations.list_open_at(Uuid::max()).unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].slug, "inv-b");
    }

    #[test]
    fn get_by_slug() {
        let (store, _dir) = setup();
        let investigations = store.investigations(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);

        let created = investigations.create("lookup-test", serde_json::json!("goal"), "r", serde_json::json!({}), Uuid::now_v7()).unwrap();

        let found = investigations.get_by_slug("lookup-test").unwrap().unwrap();
        assert_eq!(found.id, created.id);

        assert!(investigations.get_by_slug("nonexistent").unwrap().is_none());
    }
}
