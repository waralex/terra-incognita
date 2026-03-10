use std::sync::Arc;

use rocksdb::DB;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::log::LogError;

/// Maximum branch ancestry depth.
pub const MAX_BRANCH_DEPTH: usize = 8;

/// A branch record: represents a branch in the epistemic store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchRecord {
    pub id: Uuid,
    pub slug: String,
    /// Why this branch was created.
    pub reasoning: serde_json::Value,
    /// Transaction from which this branch was created. `Uuid::nil()` = genesis (branch from empty main).
    pub created_from_tx: Uuid,
    /// Precomputed ancestry: `[(branch_id, branch_point_tx)]`.
    /// First entry is self with `Uuid::max()`, last is main.
    pub ancestry: Vec<(Uuid, Uuid)>,
}

/// Low-level IO for branch storage: main CF (branch_id → body) and slug index CF (slug → branch_id).
pub struct BranchIo {
    db: Arc<DB>,
    main_cf: &'static str,
    slug_cf: &'static str,
}

impl BranchIo {
    pub(crate) fn new(db: Arc<DB>, main_cf: &'static str, slug_cf: &'static str) -> Self {
        Self { db, main_cf, slug_cf }
    }

    /// Writes a branch record + slug index atomically.
    pub fn put_with_index(&self, record: &BranchRecord) -> Result<(), LogError> {
        let main = self.main_cf()?;
        let idx = self.slug_cf()?;

        let val_bytes = serde_json::to_vec(record)
            .map_err(|e| LogError::Storage(e.to_string()))?;

        let mut batch = rocksdb::WriteBatch::default();
        batch.put_cf(main, record.id.as_bytes(), &val_bytes);
        batch.put_cf(idx, record.slug.as_bytes(), record.id.as_bytes());

        self.db
            .write(batch)
            .map_err(|e| LogError::Storage(e.to_string()))
    }

    /// Writes a branch record to the main CF (no slug index update).
    #[allow(dead_code)]
    pub fn put(&self, record: &BranchRecord) -> Result<(), LogError> {
        let main = self.main_cf()?;
        let val_bytes = serde_json::to_vec(record)
            .map_err(|e| LogError::Storage(e.to_string()))?;

        self.db
            .put_cf(main, record.id.as_bytes(), &val_bytes)
            .map_err(|e| LogError::Storage(e.to_string()))
    }

    /// Reads a branch record by UUID.
    pub fn get(&self, branch_id: &Uuid) -> Result<Option<BranchRecord>, LogError> {
        let main = self.main_cf()?;
        match self.db.get_cf(main, branch_id.as_bytes()) {
            Ok(Some(bytes)) => {
                let record: BranchRecord = serde_json::from_slice(&bytes)
                    .map_err(|e| LogError::Storage(e.to_string()))?;
                Ok(Some(record))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(LogError::Storage(e.to_string())),
        }
    }

    /// Looks up a UUID by slug from the index CF.
    pub fn get_uuid_by_slug(&self, slug: &str) -> Result<Option<Uuid>, LogError> {
        let idx = self.slug_cf()?;
        match self.db.get_cf(idx, slug.as_bytes()) {
            Ok(Some(bytes)) => {
                let uuid = Uuid::from_slice(&bytes)
                    .map_err(|e| LogError::Storage(e.to_string()))?;
                Ok(Some(uuid))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(LogError::Storage(e.to_string())),
        }
    }

    /// Scans all branch records.
    pub fn scan_all(&self) -> Result<Vec<BranchRecord>, LogError> {
        let main = self.main_cf()?;
        let mut records = Vec::new();
        let iter = self.db.iterator_cf(main, rocksdb::IteratorMode::Start);
        for item in iter {
            let (_key, val) = item.map_err(|e| LogError::Storage(e.to_string()))?;
            let record: BranchRecord = serde_json::from_slice(&val)
                .map_err(|e| LogError::Storage(e.to_string()))?;
            records.push(record);
        }
        Ok(records)
    }

    fn main_cf(&self) -> Result<&rocksdb::ColumnFamily, LogError> {
        self.db
            .cf_handle(self.main_cf)
            .ok_or_else(|| LogError::Storage(format!("missing column family: {}", self.main_cf)))
    }

    fn slug_cf(&self) -> Result<&rocksdb::ColumnFamily, LogError> {
        self.db
            .cf_handle(self.slug_cf)
            .ok_or_else(|| LogError::Storage(format!("missing column family: {}", self.slug_cf)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocksdb::{ColumnFamilyDescriptor, Options, DB};

    const MAIN_CF: &str = "branch_main";
    const SLUG_CF: &str = "branch_slug";

    fn open_db(dir: &tempfile::TempDir) -> Arc<DB> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        let cfs = vec![
            ColumnFamilyDescriptor::new(MAIN_CF, Options::default()),
            ColumnFamilyDescriptor::new(SLUG_CF, Options::default()),
        ];
        Arc::new(DB::open_cf_descriptors(&opts, dir.path(), cfs).unwrap())
    }

    fn record(id: Uuid, slug: &str) -> BranchRecord {
        BranchRecord {
            id,
            slug: slug.into(),
            reasoning: serde_json::Value::Null,
            created_from_tx: Uuid::nil(),
            ancestry: vec![(id, Uuid::max()), (Uuid::nil(), Uuid::max())],
        }
    }

    #[test]
    fn put_and_get() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let io = BranchIo::new(Arc::clone(&db), MAIN_CF, SLUG_CF);

        let id = Uuid::now_v7();
        let rec = record(id, "my-branch");
        io.put_with_index(&rec).unwrap();

        let loaded = io.get(&id).unwrap().unwrap();
        assert_eq!(loaded.id, id);
        assert_eq!(loaded.slug, "my-branch");
        assert_eq!(loaded.ancestry, vec![(id, Uuid::max()), (Uuid::nil(), Uuid::max())]);
    }

    #[test]
    fn slug_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let io = BranchIo::new(Arc::clone(&db), MAIN_CF, SLUG_CF);

        let id = Uuid::now_v7();
        io.put_with_index(&record(id, "test")).unwrap();

        assert_eq!(io.get_uuid_by_slug("test").unwrap(), Some(id));
        assert!(io.get_uuid_by_slug("nope").unwrap().is_none());
    }

    #[test]
    fn scan_all() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let io = BranchIo::new(Arc::clone(&db), MAIN_CF, SLUG_CF);

        io.put_with_index(&record(Uuid::now_v7(), "a")).unwrap();
        io.put_with_index(&record(Uuid::now_v7(), "b")).unwrap();

        let all = io.scan_all().unwrap();
        assert_eq!(all.len(), 2);
    }
}
