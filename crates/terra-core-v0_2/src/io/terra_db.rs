//! Storage backend: opens and owns the RocksDB instance.
//!
//! All access to the underlying database goes through [`TerraDb`].
//! Nothing outside this module should know about RocksDB.

use std::path::Path;
use std::sync::Arc;

use rocksdb::{ColumnFamilyDescriptor, Options, DB};

/// Column family names — single source of truth.
pub(crate) const CF_TRANSACTIONS: &str = "transactions";
pub(crate) const CF_ENTITY_MAIN: &str = "entity_main";
pub(crate) const CF_ENTITY_SLUG: &str = "entity_slug";
pub(crate) const CF_BRANCH_MAIN: &str = "branch_main";
pub(crate) const CF_BRANCH_SLUG: &str = "branch_slug";
pub(crate) const CF_SCHEMA_TYPES: &str = "schema_types";
pub(crate) const CF_SCHEMA_TYPE_SLUG: &str = "schema_type_slug";
pub(crate) const CF_SCHEMA_PROPS: &str = "schema_props";
pub(crate) const CF_SCHEMA_PROP_SLUG: &str = "schema_prop_slug";
pub(crate) const CF_SCHEMA_ATTACHMENTS: &str = "schema_attachments";
pub(crate) const CF_VISIBILITY: &str = "visibility";
pub(crate) const CF_ASSERTIONS: &str = "assertions";
pub(crate) const CF_ASSERTION_LOG: &str = "assertion_log";
pub(crate) const CF_MANAGED_MAIN: &str = "managed_main";
pub(crate) const CF_MANAGED_SLUG: &str = "managed_slug";

const ALL_CFS: &[&str] = &[
    CF_TRANSACTIONS,
    CF_ENTITY_MAIN,
    CF_ENTITY_SLUG,
    CF_BRANCH_MAIN,
    CF_BRANCH_SLUG,
    CF_SCHEMA_TYPES,
    CF_SCHEMA_TYPE_SLUG,
    CF_SCHEMA_PROPS,
    CF_SCHEMA_PROP_SLUG,
    CF_SCHEMA_ATTACHMENTS,
    CF_VISIBILITY,
    CF_ASSERTIONS,
    CF_ASSERTION_LOG,
    CF_MANAGED_MAIN,
    CF_MANAGED_SLUG,
];

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

/// Single point of access to the underlying storage.
///
/// Wraps a RocksDB instance and isolates the rest of the codebase
/// from storage engine details. If we ever swap RocksDB for something
/// else, only this module changes.
pub struct TerraDb {
    db: Arc<DB>,
    mode: AccessMode,
}

impl TerraDb {
    /// Open the database at the given path.
    pub fn open(path: &Path, mode: AccessMode) -> Result<Self, DbError> {
        let mut opts = Options::default();
        opts.create_if_missing(mode == AccessMode::ReadWrite);
        opts.create_missing_column_families(mode == AccessMode::ReadWrite);
        opts.set_prefix_extractor(rocksdb::SliceTransform::create_fixed_prefix(16));

        let cf_descriptors: Vec<ColumnFamilyDescriptor> = ALL_CFS
            .iter()
            .map(|name| {
                let mut cf_opts = Options::default();
                cf_opts.set_prefix_extractor(rocksdb::SliceTransform::create_fixed_prefix(16));
                ColumnFamilyDescriptor::new(*name, cf_opts)
            })
            .collect();

        let db = match mode {
            AccessMode::ReadWrite => DB::open_cf_descriptors(&opts, path, cf_descriptors)?,
            AccessMode::ReadOnly => {
                let cf_names: Vec<&str> = ALL_CFS.to_vec();
                DB::open_cf_for_read_only(&opts, path, &cf_names, false)?
            }
        };

        Ok(Self {
            db: Arc::new(db),
            mode,
        })
    }

    /// Access mode this database was opened with.
    pub fn mode(&self) -> AccessMode {
        self.mode
    }

    /// Get a shared reference to the inner DB.
    ///
    /// Crate-internal only — nothing outside `io` should touch this.
    pub(crate) fn inner(&self) -> &Arc<DB> {
        &self.db
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_read_write() {
        let dir = tempfile::tempdir().unwrap();
        let db = TerraDb::open(dir.path(), AccessMode::ReadWrite).unwrap();
        assert_eq!(db.mode(), AccessMode::ReadWrite);
    }

    #[test]
    fn open_read_only_after_init() {
        let dir = tempfile::tempdir().unwrap();

        // First open creates the DB and CFs.
        {
            let _db = TerraDb::open(dir.path(), AccessMode::ReadWrite).unwrap();
        }

        // Second open is read-only.
        let db = TerraDb::open(dir.path(), AccessMode::ReadOnly).unwrap();
        assert_eq!(db.mode(), AccessMode::ReadOnly);
    }

    #[test]
    fn open_read_only_without_init_fails() {
        let dir = tempfile::tempdir().unwrap();
        let err = TerraDb::open(dir.path(), AccessMode::ReadOnly);
        assert!(err.is_err());
    }
}
