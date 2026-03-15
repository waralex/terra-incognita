//! CommandState — shared mutation context for command execution.
//!
//! Holds a lazy WriteBatch that commands accumulate into.
//! The caller commits after all commands have executed,
//! giving atomic multi-command operations (e.g. checkout = branch + transaction).

use crate::io::{DbError, WriteBatch};
use crate::store::storage::Storage;

/// Shared mutation state passed to commands.
///
/// Created by the caller, passed to `Command::execute`, committed after.
/// Multiple commands can share one state for atomic composite operations.
pub struct CommandState {
    storage: Storage,
    batch: Option<WriteBatch>,
}

impl CommandState {
    /// Create a new command state bound to the given storage.
    pub fn new(storage: &Storage) -> Self {
        Self {
            storage: storage.clone(),
            batch: None,
        }
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
