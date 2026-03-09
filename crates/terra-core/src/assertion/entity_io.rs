use std::sync::Arc;

use chrono::{DateTime, Utc};
use rocksdb::DB;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::log::LogError;

/// Status of an entity at a point in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntityStatus {
    Active,
    Deleted,
}

/// A single versioned record for an entity.
#[derive(Debug, Clone, Serialize)]
pub struct EntityRecord {
    pub id: Uuid,
    pub slug: String,
    pub status: EntityStatus,
    pub description: Option<String>,
    pub timestamp: DateTime<Utc>,
}

/// Low-level IO for entity storage: main CF (uuid+timestamp → body) and slug index CF (slug → uuid).
pub struct EntityIo {
    db: Arc<DB>,
    main_cf: &'static str,
    slug_cf: &'static str,
}

impl EntityIo {
    pub(crate) fn new(db: Arc<DB>, main_cf: &'static str, slug_cf: &'static str) -> Self {
        Self { db, main_cf, slug_cf }
    }

    /// Writes an entity record to the main CF.
    pub fn put(&self, record: &EntityRecord) -> Result<(), LogError> {
        let main = self.main_cf()?;
        let key = encode_main_key(&super::MAIN_BRANCH, &record.id, record.timestamp.timestamp_micros());
        let val = serde_json::json!({
            "slug": record.slug,
            "status": record.status,
            "description": record.description,
        });
        let val_bytes = serde_json::to_vec(&val)
            .map_err(|e| LogError::Storage(e.to_string()))?;

        self.db
            .put_cf(main, &key, &val_bytes)
            .map_err(|e| LogError::Storage(e.to_string()))
    }

    /// Writes an entity record + slug index atomically.
    pub fn put_with_index(&self, record: &EntityRecord) -> Result<(), LogError> {
        let main = self.main_cf()?;
        let idx = self.slug_cf()?;

        let key = encode_main_key(&super::MAIN_BRANCH, &record.id, record.timestamp.timestamp_micros());
        let val = serde_json::json!({
            "slug": record.slug,
            "status": record.status,
            "description": record.description,
        });
        let val_bytes = serde_json::to_vec(&val)
            .map_err(|e| LogError::Storage(e.to_string()))?;

        let mut batch = rocksdb::WriteBatch::default();
        batch.put_cf(main, &key, &val_bytes);
        batch.put_cf(idx, &encode_slug_key(&super::MAIN_BRANCH, &record.slug), record.id.as_bytes());

        self.db
            .write(batch)
            .map_err(|e| LogError::Storage(e.to_string()))
    }

    /// Reads the latest record for an entity by UUID (last entry by timestamp).
    pub fn get_latest(&self, entity_id: &Uuid) -> Result<Option<EntityRecord>, LogError> {
        let main = self.main_cf()?;
        // Prefix: branch(16) | entity(16) = 32 bytes
        let mut prefix = [0u8; 32];
        prefix[0..16].copy_from_slice(super::MAIN_BRANCH.as_bytes());
        prefix[16..32].copy_from_slice(entity_id.as_bytes());

        let mut latest: Option<(i64, Vec<u8>)> = None;
        let iter = self.db.prefix_iterator_cf(main, &prefix);

        for item in iter {
            let (key, val) = item.map_err(|e| LogError::Storage(e.to_string()))?;
            if !key.starts_with(&prefix) {
                break;
            }
            let (_, _, ts) = decode_main_key(&key)?;
            match &latest {
                Some((prev_ts, _)) if ts <= *prev_ts => {}
                _ => latest = Some((ts, val.to_vec())),
            }
        }

        match latest {
            None => Ok(None),
            Some((ts, val_bytes)) => {
                let record = decode_record(entity_id, ts, &val_bytes)?;
                Ok(Some(record))
            }
        }
    }

    /// Reads all records for an entity by UUID, ordered by timestamp.
    pub fn get_history(&self, entity_id: &Uuid) -> Result<Vec<EntityRecord>, LogError> {
        let main = self.main_cf()?;
        let mut prefix = [0u8; 32];
        prefix[0..16].copy_from_slice(super::MAIN_BRANCH.as_bytes());
        prefix[16..32].copy_from_slice(entity_id.as_bytes());

        let mut records = Vec::new();
        let iter = self.db.prefix_iterator_cf(main, &prefix);

        for item in iter {
            let (key, val) = item.map_err(|e| LogError::Storage(e.to_string()))?;
            if !key.starts_with(&prefix) {
                break;
            }
            let (_, _, ts) = decode_main_key(&key)?;
            records.push(decode_record(entity_id, ts, &val)?);
        }

        Ok(records)
    }

    /// Looks up a UUID by slug from the index CF.
    pub fn get_uuid_by_slug(&self, slug: &str) -> Result<Option<Uuid>, LogError> {
        let idx = self.slug_cf()?;
        let key = encode_slug_key(&super::MAIN_BRANCH, slug);
        match self.db.get_cf(idx, &key) {
            Ok(Some(bytes)) => {
                let uuid = Uuid::from_slice(&bytes)
                    .map_err(|e| LogError::Storage(e.to_string()))?;
                Ok(Some(uuid))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(LogError::Storage(e.to_string())),
        }
    }

    /// Iterates all entries in the main CF on the main branch. Returns all latest records.
    pub fn scan_all_latest(&self) -> Result<Vec<EntityRecord>, LogError> {
        let main = self.main_cf()?;
        let branch_prefix = super::MAIN_BRANCH.as_bytes().to_vec();
        let mut latest_map: std::collections::HashMap<Uuid, (i64, Vec<u8>)> =
            std::collections::HashMap::new();

        let iter = self.db.prefix_iterator_cf(main, &branch_prefix);
        for item in iter {
            let (key, val) = item.map_err(|e| LogError::Storage(e.to_string()))?;
            if !key.starts_with(&branch_prefix) {
                break;
            }
            if key.len() < 40 {
                continue;
            }
            let (_, entity_id, ts) = decode_main_key(&key)?;

            match latest_map.get(&entity_id) {
                Some((prev_ts, _)) if ts <= *prev_ts => {}
                _ => { latest_map.insert(entity_id, (ts, val.to_vec())); }
            }
        }

        let mut records = Vec::with_capacity(latest_map.len());
        for (entity_id, (ts, val_bytes)) in latest_map {
            records.push(decode_record(&entity_id, ts, &val_bytes)?);
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

// Key: branch_id(16) | entity_uuid(16) | timestamp_us(8 BE) = 40 bytes

fn encode_main_key(branch_id: &Uuid, entity_id: &Uuid, timestamp_us: i64) -> [u8; 40] {
    let mut key = [0u8; 40];
    key[0..16].copy_from_slice(branch_id.as_bytes());
    key[16..32].copy_from_slice(entity_id.as_bytes());
    key[32..40].copy_from_slice(&timestamp_us.to_be_bytes());
    key
}

fn decode_main_key(key: &[u8]) -> Result<(Uuid, Uuid, i64), LogError> {
    if key.len() < 40 {
        return Err(LogError::Storage("entity key too short".into()));
    }
    let branch_id = Uuid::from_slice(&key[0..16])
        .map_err(|e| LogError::Storage(e.to_string()))?;
    let entity_id = Uuid::from_slice(&key[16..32])
        .map_err(|e| LogError::Storage(e.to_string()))?;
    let ts = i64::from_be_bytes(
        key[32..40]
            .try_into()
            .map_err(|_| LogError::Storage("bad timestamp".into()))?,
    );
    Ok((branch_id, entity_id, ts))
}

// Slug index key: branch_id(16) | slug_bytes
fn encode_slug_key(branch_id: &Uuid, slug: &str) -> Vec<u8> {
    let mut key = Vec::with_capacity(16 + slug.len());
    key.extend_from_slice(branch_id.as_bytes());
    key.extend_from_slice(slug.as_bytes());
    key
}

fn decode_record(entity_id: &Uuid, timestamp_us: i64, val_bytes: &[u8]) -> Result<EntityRecord, LogError> {
    let val: serde_json::Value = serde_json::from_slice(val_bytes)
        .map_err(|e| LogError::Storage(e.to_string()))?;

    let slug = val.get("slug")
        .and_then(|v| v.as_str())
        .ok_or_else(|| LogError::Storage("missing slug".into()))?
        .to_string();

    let status: EntityStatus = val.get("status")
        .map(|v| serde_json::from_value(v.clone()))
        .transpose()
        .map_err(|e| LogError::Storage(e.to_string()))?
        .unwrap_or(EntityStatus::Active);

    let description = val.get("description")
        .and_then(|v| if v.is_null() { None } else { v.as_str() })
        .map(String::from);

    let timestamp = DateTime::from_timestamp_micros(timestamp_us)
        .ok_or_else(|| LogError::Storage("invalid timestamp".into()))?;

    Ok(EntityRecord {
        id: *entity_id,
        slug,
        status,
        description,
        timestamp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocksdb::{ColumnFamilyDescriptor, Options, DB};

    const MAIN_CF: &str = "entity_main";
    const SLUG_CF: &str = "entity_slug";

    fn open_db(dir: &tempfile::TempDir) -> Arc<DB> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        let mut main_opts = Options::default();
        main_opts.set_prefix_extractor(rocksdb::SliceTransform::create_fixed_prefix(16));

        let cfs = vec![
            ColumnFamilyDescriptor::new(MAIN_CF, main_opts),
            ColumnFamilyDescriptor::new(SLUG_CF, Options::default()),
        ];
        Arc::new(DB::open_cf_descriptors(&opts, dir.path(), cfs).unwrap())
    }

    fn record(id: Uuid, slug: &str, status: EntityStatus) -> EntityRecord {
        EntityRecord {
            id,
            slug: slug.into(),
            status,
            description: None,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn put_and_get_latest() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let io = EntityIo::new(Arc::clone(&db), MAIN_CF, SLUG_CF);

        let id = Uuid::now_v7();
        let rec = record(id, "alpha", EntityStatus::Active);
        io.put(&rec).unwrap();

        let latest = io.get_latest(&id).unwrap().unwrap();
        assert_eq!(latest.id, id);
        assert_eq!(latest.slug, "alpha");
        assert_eq!(latest.status, EntityStatus::Active);
    }

    #[test]
    fn get_latest_returns_none_for_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let io = EntityIo::new(Arc::clone(&db), MAIN_CF, SLUG_CF);

        assert!(io.get_latest(&Uuid::now_v7()).unwrap().is_none());
    }

    #[test]
    fn put_with_index_and_lookup_by_slug() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let io = EntityIo::new(Arc::clone(&db), MAIN_CF, SLUG_CF);

        let id = Uuid::now_v7();
        let rec = record(id, "bravo", EntityStatus::Active);
        io.put_with_index(&rec).unwrap();

        let found = io.get_uuid_by_slug("bravo").unwrap().unwrap();
        assert_eq!(found, id);

        assert!(io.get_uuid_by_slug("nonexistent").unwrap().is_none());
    }

    #[test]
    fn history_tracks_status_changes() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let io = EntityIo::new(Arc::clone(&db), MAIN_CF, SLUG_CF);

        let id = Uuid::now_v7();
        io.put(&record(id, "charlie", EntityStatus::Active)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1));
        io.put(&record(id, "charlie", EntityStatus::Deleted)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1));
        io.put(&record(id, "charlie", EntityStatus::Active)).unwrap();

        let history = io.get_history(&id).unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].status, EntityStatus::Active);
        assert_eq!(history[1].status, EntityStatus::Deleted);
        assert_eq!(history[2].status, EntityStatus::Active);

        let latest = io.get_latest(&id).unwrap().unwrap();
        assert_eq!(latest.status, EntityStatus::Active);
    }

    #[test]
    fn scan_all_latest() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let io = EntityIo::new(Arc::clone(&db), MAIN_CF, SLUG_CF);

        let id1 = Uuid::now_v7();
        let id2 = Uuid::now_v7();

        io.put(&record(id1, "one", EntityStatus::Active)).unwrap();
        io.put(&record(id2, "two", EntityStatus::Active)).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1));
        io.put(&record(id1, "one", EntityStatus::Deleted)).unwrap();

        let all = io.scan_all_latest().unwrap();
        assert_eq!(all.len(), 2);

        let e1 = all.iter().find(|r| r.id == id1).unwrap();
        assert_eq!(e1.status, EntityStatus::Deleted);

        let e2 = all.iter().find(|r| r.id == id2).unwrap();
        assert_eq!(e2.status, EntityStatus::Active);
    }

    #[test]
    fn key_encoding_roundtrip() {
        let branch = Uuid::nil();
        let id = Uuid::now_v7();
        let ts: i64 = 1_700_000_000_000_000;
        let key = encode_main_key(&branch, &id, ts);
        assert_eq!(key.len(), 40);

        let (bid, eid, decoded_ts) = decode_main_key(&key).unwrap();
        assert_eq!(bid, branch);
        assert_eq!(eid, id);
        assert_eq!(decoded_ts, ts);
    }
}
