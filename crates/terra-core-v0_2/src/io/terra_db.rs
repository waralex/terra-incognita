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

use rocksdb::{ColumnFamilyDescriptor, Direction, IteratorMode, Options, ReadOptions, DB};

use crate::io::db_item::DbItem;
use crate::io::db_iterator::DbIterator;
use crate::io::storage_key::StorageKey;
use crate::io::storage_value::StorageValue;
use crate::io::valid_prefix::ValidPrefix;
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

impl From<crate::io::storage_key::KeyError> for DbError {
    fn from(e: crate::io::storage_key::KeyError) -> Self {
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

    /// Get an item by its typed key.
    pub fn get<T: DbItem>(&self, key: &T::Key) -> Result<Option<T>, DbError> {
        let cf = self.db.cf_handle(T::cf())
            .ok_or_else(|| DbError::Storage(format!("missing column family: {}", T::cf())))?;
        let key_bytes = key.encode();
        match self.db.get_cf(cf, &key_bytes) {
            Ok(Some(val_bytes)) => {
                let k = T::Key::decode(&key_bytes)?;
                let v = T::Value::decode(&val_bytes)?;
                Ok(Some(T::from_parts(k, v)))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(DbError::Storage(e.to_string())),
        }
    }

    /// Iterate forward over items whose keys share the given prefix.
    pub fn scan<'a, T: DbItem>(
        &'a self,
        prefix: &impl ValidPrefix<T>,
    ) -> Result<DbIterator<'a, T>, DbError> {
        let cf = self.db.cf_handle(T::cf())
            .ok_or_else(|| DbError::Storage(format!("missing column family: {}", T::cf())))?;
        let prefix_bytes = prefix.encode();
        let opts = ReadOptions::default();
        let mode = IteratorMode::From(&prefix_bytes, Direction::Forward);
        let inner = self.db.iterator_cf_opt(cf, opts, mode);
        Ok(DbIterator::new(inner, prefix_bytes))
    }

    /// Iterate in reverse over items whose keys share the given prefix.
    ///
    /// Yields entries from the largest key down to the smallest within
    /// the prefix range — useful for finding the latest version.
    pub fn scan_rev<'a, T: DbItem>(
        &'a self,
        prefix: &impl ValidPrefix<T>,
    ) -> Result<DbIterator<'a, T>, DbError> {
        let cf = self.db.cf_handle(T::cf())
            .ok_or_else(|| DbError::Storage(format!("missing column family: {}", T::cf())))?;
        let prefix_bytes = prefix.encode();
        let opts = ReadOptions::default();
        let mut ceiling = prefix_bytes.clone();
        let pad = T::Key::SIZE.saturating_sub(prefix_bytes.len());
        ceiling.extend(std::iter::repeat(0xFFu8).take(pad));
        let mode = IteratorMode::From(&ceiling, Direction::Reverse);
        let inner = self.db.iterator_cf_opt(cf, opts, mode);
        Ok(DbIterator::new(inner, prefix_bytes))
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

    use crate::io::storage_key::KeyError;
    use crate::io::storage_value::StorageValue;
    use crate::io::storage_key::StorageKey;

    #[derive(Debug, Clone)]
    struct TestKey;

    impl StorageKey for TestKey {
        const SIZE: usize = 0;
        fn encode(&self) -> Vec<u8> { vec![] }
        fn decode(_bytes: &[u8]) -> Result<Self, KeyError> { Ok(TestKey) }
    }

    #[derive(Debug, Clone)]
    struct TestValue;

    impl StorageValue for TestValue {
        fn encode(&self) -> Result<Vec<u8>, DbError> { Ok(vec![]) }
        fn decode(_bytes: &[u8]) -> Result<Self, DbError> { Ok(TestValue) }
    }

    struct TestItem;

    impl DbItem for TestItem {
        type Key = TestKey;
        type Value = TestValue;

        fn cf() -> &'static str { "test_cf" }
        fn key(&self) -> &TestKey { &TestKey }
        fn value(&self) -> &TestValue { &TestValue }
        fn from_parts(_key: TestKey, _value: TestValue) -> Self { TestItem }
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

    mod scan_tests {
        use uuid::Uuid;
        use crate::io::TerraDb;
        use crate::io::storage_key::storage_key;
        use crate::io::valid_prefix::impl_prefix;
        use crate::store::entity_entry::{EntityEntry, EntityKey, EntityValue};
        use crate::store::prefix::BranchPrefix;

        storage_key! {
            pub struct BranchEntityPrefix(32) {
                branch_id: Uuid,
                entity_id: Uuid,
            }
        }

        impl_prefix!(BranchEntityPrefix => EntityEntry);

        fn write_entity(db: &TerraDb, branch_id: Uuid, entity_id: Uuid, tx_id: Uuid, slug: &str) {
            let entry = EntityEntry {
                key: EntityKey { branch_id, entity_id, tx_id },
                value: EntityValue {
                    slug: slug.into(),
                    entity_type_id: Uuid::nil(),
                    description: None,
                },
            };
            let mut batch = db.batch();
            batch.put(&entry).unwrap();
            batch.commit().unwrap();
        }

        #[test]
        fn scan_forward() {
            let dir = tempfile::tempdir().unwrap();
            let db = TerraDb::builder(dir.path())
                .with::<EntityEntry>()
                .open()
                .unwrap();

            let branch = Uuid::from_u128(1);
            let entity = Uuid::from_u128(2);
            let tx1 = Uuid::from_u128(10);
            let tx2 = Uuid::from_u128(20);
            let tx3 = Uuid::from_u128(30);

            write_entity(&db, branch, entity, tx1, "v1");
            write_entity(&db, branch, entity, tx2, "v2");
            write_entity(&db, branch, entity, tx3, "v3");

            let prefix = BranchEntityPrefix { branch_id: branch, entity_id: entity };
            let items: Vec<EntityEntry> = db.scan::<EntityEntry>(&prefix)
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();

            assert_eq!(items.len(), 3);
            assert_eq!(items[0].value.slug, "v1");
            assert_eq!(items[1].value.slug, "v2");
            assert_eq!(items[2].value.slug, "v3");
        }

        #[test]
        fn scan_reverse() {
            let dir = tempfile::tempdir().unwrap();
            let db = TerraDb::builder(dir.path())
                .with::<EntityEntry>()
                .open()
                .unwrap();

            let branch = Uuid::from_u128(1);
            let entity = Uuid::from_u128(2);
            let tx1 = Uuid::from_u128(10);
            let tx2 = Uuid::from_u128(20);
            let tx3 = Uuid::from_u128(30);

            write_entity(&db, branch, entity, tx1, "v1");
            write_entity(&db, branch, entity, tx2, "v2");
            write_entity(&db, branch, entity, tx3, "v3");

            let prefix = BranchEntityPrefix { branch_id: branch, entity_id: entity };
            let items: Vec<EntityEntry> = db.scan_rev::<EntityEntry>(&prefix)
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();

            assert_eq!(items.len(), 3);
            assert_eq!(items[0].value.slug, "v3");
            assert_eq!(items[1].value.slug, "v2");
            assert_eq!(items[2].value.slug, "v1");
        }

        #[test]
        fn scan_prefix_isolates_entities() {
            let dir = tempfile::tempdir().unwrap();
            let db = TerraDb::builder(dir.path())
                .with::<EntityEntry>()
                .open()
                .unwrap();

            let branch = Uuid::from_u128(1);
            let e1 = Uuid::from_u128(100);
            let e2 = Uuid::from_u128(200);
            let tx = Uuid::from_u128(10);

            write_entity(&db, branch, e1, tx, "entity-1");
            write_entity(&db, branch, e2, tx, "entity-2");

            let prefix = BranchEntityPrefix { branch_id: branch, entity_id: e1 };
            let items: Vec<EntityEntry> = db.scan::<EntityEntry>(&prefix)
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();

            assert_eq!(items.len(), 1);
            assert_eq!(items[0].value.slug, "entity-1");
        }

        #[test]
        fn scan_empty_prefix_range() {
            let dir = tempfile::tempdir().unwrap();
            let db = TerraDb::builder(dir.path())
                .with::<EntityEntry>()
                .open()
                .unwrap();

            let branch = Uuid::from_u128(1);
            let entity = Uuid::from_u128(999);

            let prefix = BranchEntityPrefix { branch_id: branch, entity_id: entity };
            let items: Vec<EntityEntry> = db.scan::<EntityEntry>(&prefix)
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();

            assert!(items.is_empty());
        }

        #[test]
        fn scan_with_filter() {
            let dir = tempfile::tempdir().unwrap();
            let db = TerraDb::builder(dir.path())
                .with::<EntityEntry>()
                .open()
                .unwrap();

            let branch = Uuid::from_u128(1);
            let entity = Uuid::from_u128(2);
            let tx1 = Uuid::from_u128(10);
            let tx2 = Uuid::from_u128(20);
            let tx3 = Uuid::from_u128(30);

            write_entity(&db, branch, entity, tx1, "v1");
            write_entity(&db, branch, entity, tx2, "v2");
            write_entity(&db, branch, entity, tx3, "v3");

            let bound = Uuid::from_u128(25);
            let prefix = BranchEntityPrefix { branch_id: branch, entity_id: entity };
            let items: Vec<EntityEntry> = db.scan::<EntityEntry>(&prefix)
                .unwrap()
                .filter_map(|r| {
                    let e = r.ok()?;
                    (e.key.tx_id <= bound).then_some(e)
                })
                .collect();

            assert_eq!(items.len(), 2);
            assert_eq!(items[0].value.slug, "v1");
            assert_eq!(items[1].value.slug, "v2");
        }

        #[test]
        fn scan_rev_latest_version() {
            let dir = tempfile::tempdir().unwrap();
            let db = TerraDb::builder(dir.path())
                .with::<EntityEntry>()
                .open()
                .unwrap();

            let branch = Uuid::from_u128(1);
            let entity = Uuid::from_u128(2);
            let tx1 = Uuid::from_u128(10);
            let tx2 = Uuid::from_u128(20);
            let tx3 = Uuid::from_u128(30);

            write_entity(&db, branch, entity, tx1, "v1");
            write_entity(&db, branch, entity, tx2, "v2");
            write_entity(&db, branch, entity, tx3, "v3");

            let bound = Uuid::from_u128(25);
            let prefix = BranchEntityPrefix { branch_id: branch, entity_id: entity };
            let latest = db.scan_rev::<EntityEntry>(&prefix)
                .unwrap()
                .filter_map(|r| r.ok())
                .find(|e| e.key.tx_id <= bound);

            assert!(latest.is_some());
            assert_eq!(latest.unwrap().value.slug, "v2");
        }

        #[test]
        fn scan_broader_prefix() {
            let dir = tempfile::tempdir().unwrap();
            let db = TerraDb::builder(dir.path())
                .with::<EntityEntry>()
                .open()
                .unwrap();

            let branch = Uuid::from_u128(1);
            let e1 = Uuid::from_u128(100);
            let e2 = Uuid::from_u128(200);

            write_entity(&db, branch, e1, Uuid::from_u128(10), "a");
            write_entity(&db, branch, e2, Uuid::from_u128(20), "b");

            let prefix = BranchPrefix { branch_id: branch };
            let items: Vec<EntityEntry> = db.scan::<EntityEntry>(&prefix)
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();

            assert_eq!(items.len(), 2);
        }
    }
}
