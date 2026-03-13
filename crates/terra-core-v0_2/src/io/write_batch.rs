//! Atomic write batch — groups multiple writes into a single atomic operation.

use std::sync::Arc;

use rocksdb::DB;

use crate::io::db_item::DbItem;
use crate::io::terra_db::DbError;

/// Atomic batch of writes bound to a specific database.
/// Accumulates operations and commits them all-or-nothing.
///
/// Created via [`TerraDb::batch`]. Consumes itself on commit
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

    /// Add an item to the batch.
    pub fn put<T: DbItem>(&mut self, item: &T) -> Result<(), DbError> {
        let cf = self.db.cf_handle(T::cf())
            .ok_or_else(|| DbError::Storage(format!("missing column family: {}", T::cf())))?;
        let key = item.encode_key();
        let value = item.encode_value()?;
        self.inner.put_cf(cf, &key, &value);
        Ok(())
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
    use crate::io::TerraDb;

    #[test]
    fn commit_empty_batch() {
        let dir = tempfile::tempdir().unwrap();
        let db = TerraDb::builder(dir.path()).open().unwrap();
        db.batch().commit().unwrap();
    }
}
