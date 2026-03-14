//! Storage backend: opens and owns the RocksDB instance.
//!
//! All access to the underlying database goes through [`TerraDb`].
//! Nothing outside this module should know about RocksDB.
//!
//! # Example
//!
//! ```ignore
//! let db = TerraDb::builder(path)
//!     .with::<EntityRecord>()
//!     .with::<BranchRecord>()
//!     .read_only()
//!     .open()?;
//! ```

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rocksdb::{ColumnFamilyDescriptor, Options, DB};

use crate::io::db_item::DbItem;
use crate::io::write_batch::WriteBatch;

/// Access mode for the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessMode {
    /// Full read-write access.
    ReadWrite,
    /// Read-only — no writes allowed, safe for concurrent readers.
    ReadOnly,
}

/// Storage errors.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("storage error: {0}")]
    Storage(String),
}

impl From<rocksdb::Error> for DbError {
    fn from(e: rocksdb::Error) -> Self {
        DbError::Storage(e.to_string())
    }
}

/// Builder for [`TerraDb`]. Collects column families from registered
/// types and opens the database.
pub struct TerraDbBuilder {
    path: PathBuf,
    mode: AccessMode,
    cf_names: BTreeSet<String>,
}

impl TerraDbBuilder {
    /// Register a [`DbItem`] type — its column family will be created on open.
    pub fn with<T: DbItem>(mut self) -> Self {
        self.cf_names.insert(T::cf().to_string());
        self
    }

    /// Set read-only mode. Default is read-write.
    pub fn read_only(mut self) -> Self {
        self.mode = AccessMode::ReadOnly;
        self
    }

    /// Open the database with all registered column families.
    pub fn open(self) -> Result<TerraDb, DbError> {
        TerraDb::open_internal(&self.path, self.mode, &self.cf_names)
    }
}

/// Single point of access to the underlying storage.
///
/// Wraps a RocksDB instance and isolates the rest of the codebase
/// from storage engine details. If we ever swap RocksDB for something
/// else, only this module changes.
#[derive(Clone)]
pub struct TerraDb {
    pub(super) db: Arc<DB>,
    mode: AccessMode,
}

impl TerraDb {
    /// Create a builder for opening a database at the given path.
    pub fn builder(path: &Path) -> TerraDbBuilder {
        TerraDbBuilder {
            path: path.to_path_buf(),
            mode: AccessMode::ReadWrite,
            cf_names: BTreeSet::new(),
        }
    }

    /// Access mode this database was opened with.
    pub fn mode(&self) -> AccessMode {
        self.mode
    }

    /// Get an item by its key.
    pub fn get<T: DbItem>(&self, key: &[u8]) -> Result<Option<T>, DbError> {
        let cf = self.db.cf_handle(T::cf())
            .ok_or_else(|| DbError::Storage(format!("missing column family: {}", T::cf())))?;
        match self.db.get_cf(cf, key) {
            Ok(Some(value)) => Ok(Some(T::decode(key, &value)?)),
            Ok(None) => Ok(None),
            Err(e) => Err(DbError::Storage(e.to_string())),
        }
    }

    /// Create a new write batch bound to this database.
    pub fn batch(&self) -> WriteBatch {
        WriteBatch::new(Arc::clone(&self.db))
    }

    fn open_internal(path: &Path, mode: AccessMode, cf_names: &BTreeSet<String>) -> Result<Self, DbError> {
        let mut opts = Options::default();
        opts.create_if_missing(mode == AccessMode::ReadWrite);
        opts.create_missing_column_families(mode == AccessMode::ReadWrite);
        opts.set_prefix_extractor(rocksdb::SliceTransform::create_fixed_prefix(16));

        let cf_descriptors: Vec<ColumnFamilyDescriptor> = cf_names
            .iter()
            .map(|name| {
                let mut cf_opts = Options::default();
                cf_opts.set_prefix_extractor(rocksdb::SliceTransform::create_fixed_prefix(16));
                ColumnFamilyDescriptor::new(name.as_str(), cf_opts)
            })
            .collect();

        let names: Vec<&str> = cf_names.iter().map(|s| s.as_str()).collect();

        let db = match mode {
            AccessMode::ReadWrite => DB::open_cf_descriptors(&opts, path, cf_descriptors)?,
            AccessMode::ReadOnly => DB::open_cf_for_read_only(&opts, path, &names, false)?,
        };

        Ok(Self {
            db: Arc::new(db),
            mode,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestItem;

    impl DbItem for TestItem {
        fn cf() -> &'static str { "test_cf" }
        fn encode_key(&self) -> Vec<u8> { vec![] }
        fn encode_value(&self) -> Result<Vec<u8>, DbError> { Ok(vec![]) }
        fn decode(_key: &[u8], _value: &[u8]) -> Result<Self, DbError> { Ok(TestItem) }
    }

    #[test]
    fn builder_read_write() {
        let dir = tempfile::tempdir().unwrap();
        let db = TerraDb::builder(dir.path())
            .with::<TestItem>()
            .open()
            .unwrap();
        assert_eq!(db.mode(), AccessMode::ReadWrite);
    }

    #[test]
    fn builder_read_only_after_init() {
        let dir = tempfile::tempdir().unwrap();
        {
            let _db = TerraDb::builder(dir.path())
                .with::<TestItem>()
                .open()
                .unwrap();
        }
        let db = TerraDb::builder(dir.path())
            .with::<TestItem>()
            .read_only()
            .open()
            .unwrap();
        assert_eq!(db.mode(), AccessMode::ReadOnly);
    }

    #[test]
    fn builder_read_only_without_init_fails() {
        let dir = tempfile::tempdir().unwrap();
        let err = TerraDb::builder(dir.path())
            .with::<TestItem>()
            .read_only()
            .open();
        assert!(err.is_err());
    }

    #[test]
    fn builder_no_cfs() {
        let dir = tempfile::tempdir().unwrap();
        let db = TerraDb::builder(dir.path()).open().unwrap();
        assert_eq!(db.mode(), AccessMode::ReadWrite);
    }
}
