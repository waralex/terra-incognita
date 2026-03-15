//! CommandState — shared mutation context for command execution.
//!
//! Holds a lazy WriteBatch that commands accumulate into.
//! The caller commits after all commands have executed,
//! giving atomic multi-command operations (e.g. checkout = branch + transaction).

use std::sync::Arc;

use crate::embed::{Embedder, NoopEmbedder};
use crate::io::{DbError, WriteBatch};
use crate::store::storage::Storage;

/// Shared mutation state passed to commands.
///
/// Created by the caller, passed to `Command::execute`, committed after.
/// Multiple commands can share one state for atomic composite operations.
pub struct CommandState {
    storage: Storage,
    batch: Option<WriteBatch>,
    embedder: Arc<dyn Embedder>,
}

impl CommandState {
    /// Create a new command state bound to the given storage.
    pub fn new(storage: &Storage) -> Self {
        Self {
            storage: storage.clone(),
            batch: None,
            embedder: Arc::new(NoopEmbedder),
        }
    }

    /// Create a new command state with a custom embedder.
    pub fn with_embedder(storage: &Storage, embedder: Arc<dyn Embedder>) -> Self {
        Self {
            storage: storage.clone(),
            batch: None,
            embedder,
        }
    }

    /// Access the embedder.
    pub fn embedder(&self) -> &dyn Embedder {
        &*self.embedder
    }

    /// Get the write batch, creating it lazily on first access.
    pub fn batch(&mut self) -> &mut WriteBatch {
        if self.batch.is_none() {
            self.batch = Some(self.storage.batch());
        }
        self.batch.as_mut().unwrap()
    }

    /// Commit the accumulated writes atomically. No-op if no writes were made.
    pub fn commit(self) -> Result<(), DbError> {
        if let Some(batch) = self.batch {
            batch.commit()?;
        }
        Ok(())
    }
}
