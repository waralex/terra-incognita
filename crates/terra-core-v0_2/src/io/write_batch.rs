//! Atomic write batch — groups multiple writes into a single atomic operation.

use std::sync::Arc;

use rocksdb::DB;

use super::terra_db::DbError;

/// Atomic batch of writes bound to a specific database.
/// Accumulates operations and commits them all-or-nothing.
///
/// Created via [`super::TerraDb::batch`]. Consumes itself on commit
/// to prevent reuse after writing.
pub struct WriteBatch {
    db: Arc<DB>,
    pub(super) inner: rocksdb::WriteBatch,
}

impl WriteBatch {
    pub(super) fn new(db: Arc<DB>) -> Self {
        Self {
            db,
            inner: rocksdb::WriteBatch::default(),
        }
    }

    /// Commit all accumulated operations atomically. Consumes the batch.
    pub fn commit(self) -> Result<(), DbError> {
        self.db
            .write(self.inner)
            .map_err(|e| DbError::Storage(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use crate::io::{AccessMode, TerraDb};

    #[test]
    fn commit_empty_batch() {
        let dir = tempfile::tempdir().unwrap();
        let db = TerraDb::open(dir.path(), AccessMode::ReadWrite).unwrap();
        db.batch().commit().unwrap();
    }
}
