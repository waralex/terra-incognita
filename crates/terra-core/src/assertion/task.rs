use uuid::Uuid;

use super::task_io::{TaskIo, TaskRecord, TaskStatus};
use super::log::LogError;

/// Errors from task operations.
#[derive(Debug, thiserror::Error)]
pub enum TaskError {
    /// Slug already taken by another task.
    #[error("task slug already exists: {0}")]
    SlugExists(String),

    /// Task not found by UUID.
    #[error("task not found: {0}")]
    NotFound(Uuid),

    /// Task not found by slug.
    #[error("task not found by slug: {0}")]
    SlugNotFound(String),

    /// Task is already in the requested status.
    #[error("task {0} is already {1}")]
    AlreadyInStatus(Uuid, &'static str),

    /// Storage-level error.
    #[error(transparent)]
    Storage(#[from] LogError),
}

/// Mid-level task operations: create, update notes, close, list.
///
/// Branch-aware: writes go to the configured branch, reads walk the ancestry chain.
pub struct TaskStore {
    io: TaskIo,
}

impl TaskStore {
    pub(crate) fn new(io: TaskIo) -> Self {
        Self { io }
    }

    /// Creates a new task. Generates UUID, writes record + slug index.
    pub fn create(
        &self,
        slug: &str,
        goal: serde_json::Value,
        reasoning: &str,
        context: serde_json::Value,
        kind: Option<&str>,
        tx_id: Uuid,
    ) -> Result<TaskRecord, TaskError> {
        if self.io.get_uuid_by_slug(slug)?.is_some() {
            return Err(TaskError::SlugExists(slug.to_string()));
        }

        let record = TaskRecord {
            id: Uuid::now_v7(),
            slug: slug.to_string(),
            status: TaskStatus::Open,
            goal,
            reasoning: reasoning.to_string(),
            context,
            kind: kind.map(String::from),
            notes: serde_json::Value::Null,
            resolution: None,
            tx_id,
        };

        self.io.put_with_index(&record)?;
        Ok(record)
    }

    /// Updates the notes on an open task.
    pub fn update_notes(
        &self,
        task_id: &Uuid,
        notes: serde_json::Value,
        tx_id: Uuid,
    ) -> Result<TaskRecord, TaskError> {
        let current = self.require(task_id)?;

        if current.status == TaskStatus::Closed {
            return Err(TaskError::AlreadyInStatus(*task_id, "closed"));
        }

        let record = TaskRecord {
            notes,
            tx_id,
            ..current
        };

        self.io.put(&record)?;
        Ok(record)
    }

    /// Closes an task with a resolution.
    pub fn close(
        &self,
        task_id: &Uuid,
        resolution: serde_json::Value,
        tx_id: Uuid,
    ) -> Result<TaskRecord, TaskError> {
        let current = self.require(task_id)?;

        if current.status == TaskStatus::Closed {
            return Err(TaskError::AlreadyInStatus(*task_id, "closed"));
        }

        let record = TaskRecord {
            status: TaskStatus::Closed,
            resolution: Some(resolution),
            tx_id,
            ..current
        };

        self.io.put(&record)?;
        Ok(record)
    }

    /// Gets the current state of an task by UUID.
    pub fn get(&self, task_id: &Uuid) -> Result<Option<TaskRecord>, TaskError> {
        Ok(self.io.get_latest(task_id)?)
    }

    /// Gets the current state of an task by slug.
    pub fn get_by_slug(&self, slug: &str) -> Result<Option<TaskRecord>, TaskError> {
        let uuid = match self.io.get_uuid_by_slug(slug)? {
            Some(id) => id,
            None => return Ok(None),
        };
        Ok(self.io.get_latest(&uuid)?)
    }

    /// Lists all tasks with their current state, bounded by tx_id.
    pub fn list_all_at(&self, upper_bound: Uuid) -> Result<Vec<TaskRecord>, TaskError> {
        Ok(self.io.scan_all_latest_at(upper_bound)?)
    }

    /// Lists only currently open tasks.
    pub fn list_open_at(&self, upper_bound: Uuid) -> Result<Vec<TaskRecord>, TaskError> {
        let all = self.io.scan_all_latest_at(upper_bound)?;
        Ok(all.into_iter().filter(|r| r.status == TaskStatus::Open).collect())
    }

    fn require(&self, task_id: &Uuid) -> Result<TaskRecord, TaskError> {
        self.io.get_latest(task_id)?
            .ok_or_else(|| TaskError::NotFound(*task_id))
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
    fn create_task() {
        let (store, _dir) = setup();
        let tasks = store.tasks(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);

        let rec = tasks.create(
            "explore-orders",
            serde_json::json!("Understand the orders table"),
            "Starting exploration",
            serde_json::json!({}),
            None,
            Uuid::now_v7(),
        ).unwrap();

        assert_eq!(rec.slug, "explore-orders");
        assert_eq!(rec.status, TaskStatus::Open);
        assert!(rec.resolution.is_none());
    }

    #[test]
    fn duplicate_slug_fails() {
        let (store, _dir) = setup();
        let tasks = store.tasks(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);

        tasks.create("alpha", serde_json::json!("goal"), "r", serde_json::json!({}), None, Uuid::now_v7()).unwrap();
        let err = tasks.create("alpha", serde_json::json!("goal2"), "r", serde_json::json!({}), None, Uuid::now_v7()).unwrap_err();
        assert!(matches!(err, TaskError::SlugExists(_)));
    }

    #[test]
    fn update_notes() {
        let (store, _dir) = setup();
        let tasks = store.tasks(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);

        let rec = tasks.create("inv1", serde_json::json!("goal"), "r", serde_json::json!({}), None, Uuid::now_v7()).unwrap();

        let updated = tasks.update_notes(
            &rec.id,
            serde_json::json!({"finding": "interesting pattern"}),
            Uuid::now_v7(),
        ).unwrap();

        assert_eq!(updated.notes, serde_json::json!({"finding": "interesting pattern"}));
        assert_eq!(updated.status, TaskStatus::Open);
    }

    #[test]
    fn close_task() {
        let (store, _dir) = setup();
        let tasks = store.tasks(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);

        let rec = tasks.create("inv2", serde_json::json!("goal"), "r", serde_json::json!({}), None, Uuid::now_v7()).unwrap();

        let closed = tasks.close(
            &rec.id,
            serde_json::json!({"conclusion": "done"}),
            Uuid::now_v7(),
        ).unwrap();

        assert_eq!(closed.status, TaskStatus::Closed);
        assert_eq!(closed.resolution.unwrap(), serde_json::json!({"conclusion": "done"}));
    }

    #[test]
    fn close_already_closed_fails() {
        let (store, _dir) = setup();
        let tasks = store.tasks(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);

        let rec = tasks.create("inv3", serde_json::json!("goal"), "r", serde_json::json!({}), None, Uuid::now_v7()).unwrap();
        tasks.close(&rec.id, serde_json::json!("done"), Uuid::now_v7()).unwrap();

        let err = tasks.close(&rec.id, serde_json::json!("again"), Uuid::now_v7()).unwrap_err();
        assert!(matches!(err, TaskError::AlreadyInStatus(_, "closed")));
    }

    #[test]
    fn list_open_excludes_closed() {
        let (store, _dir) = setup();
        let tasks = store.tasks(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);

        let a = tasks.create("inv-a", serde_json::json!("goal"), "r", serde_json::json!({}), None, Uuid::now_v7()).unwrap();
        tasks.create("inv-b", serde_json::json!("goal"), "r", serde_json::json!({}), None, Uuid::now_v7()).unwrap();
        tasks.close(&a.id, serde_json::json!("done"), Uuid::now_v7()).unwrap();

        let open = tasks.list_open_at(Uuid::max()).unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].slug, "inv-b");
    }

    #[test]
    fn get_by_slug() {
        let (store, _dir) = setup();
        let tasks = store.tasks(MAIN_BRANCH, vec![(MAIN_BRANCH, Uuid::max())]);

        let created = tasks.create("lookup-test", serde_json::json!("goal"), "r", serde_json::json!({}), None, Uuid::now_v7()).unwrap();

        let found = tasks.get_by_slug("lookup-test").unwrap().unwrap();
        assert_eq!(found.id, created.id);

        assert!(tasks.get_by_slug("nonexistent").unwrap().is_none());
    }
}
