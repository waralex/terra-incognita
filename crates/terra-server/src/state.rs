use std::path::PathBuf;
use std::sync::Arc;
use terra_core::assertion::AssertionStore;

/// Shared application state holding the DB path.
/// Re-opens the store on each request to pick up writes from the agent.
pub struct Inner {
    pub db_path: PathBuf,
}

impl Inner {
    /// Opens a fresh read-only store snapshot.
    pub fn open_store(&self) -> AssertionStore {
        AssertionStore::open_read_only(&self.db_path)
            .expect("failed to open assertion store")
    }
}

/// Thread-safe shared application state.
pub type AppState = Arc<Inner>;
