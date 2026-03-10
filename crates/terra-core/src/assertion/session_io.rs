use std::sync::Arc;

use chrono::{DateTime, Utc};
use rocksdb::DB;
use serde::Serialize;
use uuid::Uuid;

use super::key::{storage_key, StorageKey};
use super::log::LogError;

/// A session record: a view over entity space scoped to certain types and entities.
#[derive(Debug, Clone, Serialize)]
pub struct SessionRecord {
    pub id: Uuid,
    pub slug: String,
    pub description: Option<String>,
    /// Allowed entity types (UUIDs from schema registry).
    pub entity_types: Vec<Uuid>,
    /// Entities added at session creation.
    pub seed_entities: Vec<Uuid>,
    /// Entities created within this session.
    pub introduced_entities: Vec<Uuid>,
    pub timestamp: DateTime<Utc>,
}

/// Low-level IO for session storage: main CF (uuid+timestamp → body) and slug index CF (slug → uuid).
pub struct SessionIo {
    db: Arc<DB>,
    main_cf: &'static str,
    slug_cf: &'static str,
}

impl SessionIo {
    pub(crate) fn new(db: Arc<DB>, main_cf: &'static str, slug_cf: &'static str) -> Self {
        Self { db, main_cf, slug_cf }
    }

    /// Writes a session record + slug index atomically.
    pub fn put_with_index(&self, record: &SessionRecord) -> Result<(), LogError> {
        let main = self.main_cf()?;
        let idx = self.slug_cf()?;

        let key = SessionKey {
            session_id: record.id,
            timestamp_us: record.timestamp.timestamp_micros(),
        }
        .encode();
        let val_bytes = self.serialize_record(record)?;

        let mut batch = rocksdb::WriteBatch::default();
        batch.put_cf(main, &key, &val_bytes);
        batch.put_cf(
            idx,
            &encode_slug_key(&record.slug),
            record.id.as_bytes(),
        );

        self.db
            .write(batch)
            .map_err(|e| LogError::Storage(e.to_string()))
    }

    /// Writes a session record to the main CF (no slug index update).
    pub fn put(&self, record: &SessionRecord) -> Result<(), LogError> {
        let main = self.main_cf()?;
        let key = SessionKey {
            session_id: record.id,
            timestamp_us: record.timestamp.timestamp_micros(),
        }
        .encode();
        let val_bytes = self.serialize_record(record)?;

        self.db
            .put_cf(main, &key, &val_bytes)
            .map_err(|e| LogError::Storage(e.to_string()))
    }

    /// Reads the latest record for a session by UUID.
    pub fn get_latest(&self, session_id: &Uuid) -> Result<Option<SessionRecord>, LogError> {
        let main = self.main_cf()?;
        let prefix = SessionKey::prefix_session(session_id);

        let mut latest: Option<(i64, Vec<u8>)> = None;
        let iter = self.db.prefix_iterator_cf(main, &prefix);

        for item in iter {
            let (raw_key, val) = item.map_err(|e| LogError::Storage(e.to_string()))?;
            if !raw_key.starts_with(&prefix) {
                break;
            }
            let k = SessionKey::decode(&raw_key)?;
            match &latest {
                Some((prev_ts, _)) if k.timestamp_us <= *prev_ts => {}
                _ => latest = Some((k.timestamp_us, val.to_vec())),
            }
        }

        match latest {
            None => Ok(None),
            Some((_ts, val_bytes)) => {
                let record = self.decode_record(session_id, &val_bytes)?;
                Ok(Some(record))
            }
        }
    }

    /// Looks up a UUID by slug from the index CF.
    pub fn get_uuid_by_slug(&self, slug: &str) -> Result<Option<Uuid>, LogError> {
        let idx = self.slug_cf()?;
        let key = encode_slug_key(slug);
        match self.db.get_cf(idx, &key) {
            Ok(Some(bytes)) => {
                let uuid =
                    Uuid::from_slice(&bytes).map_err(|e| LogError::Storage(e.to_string()))?;
                Ok(Some(uuid))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(LogError::Storage(e.to_string())),
        }
    }

    /// Scans all sessions, returning latest record for each.
    pub fn scan_all_latest(&self) -> Result<Vec<SessionRecord>, LogError> {
        let main = self.main_cf()?;

        let mut latest_map: std::collections::HashMap<Uuid, Vec<u8>> =
            std::collections::HashMap::new();

        let iter = self.db.iterator_cf(main, rocksdb::IteratorMode::Start);
        for item in iter {
            let (raw_key, val) = item.map_err(|e| LogError::Storage(e.to_string()))?;
            if raw_key.len() < SessionKey::SIZE {
                continue;
            }
            let k = SessionKey::decode(&raw_key)?;
            latest_map.insert(k.session_id, val.to_vec());
        }

        let mut records = Vec::with_capacity(latest_map.len());
        for (session_id, val_bytes) in latest_map {
            records.push(self.decode_record(&session_id, &val_bytes)?);
        }
        Ok(records)
    }

    fn serialize_record(&self, record: &SessionRecord) -> Result<Vec<u8>, LogError> {
        let val = serde_json::json!({
            "slug": record.slug,
            "description": record.description,
            "entity_types": record.entity_types.iter().map(|u| u.to_string()).collect::<Vec<_>>(),
            "seed_entities": record.seed_entities.iter().map(|u| u.to_string()).collect::<Vec<_>>(),
            "introduced_entities": record.introduced_entities.iter().map(|u| u.to_string()).collect::<Vec<_>>(),
        });
        serde_json::to_vec(&val).map_err(|e| LogError::Storage(e.to_string()))
    }

    fn decode_record(
        &self,
        session_id: &Uuid,
        val_bytes: &[u8],
    ) -> Result<SessionRecord, LogError> {
        let val: serde_json::Value =
            serde_json::from_slice(val_bytes).map_err(|e| LogError::Storage(e.to_string()))?;

        let slug = val
            .get("slug")
            .and_then(|v| v.as_str())
            .ok_or_else(|| LogError::Storage("missing slug".into()))?
            .to_string();

        let description = val
            .get("description")
            .and_then(|v| if v.is_null() { None } else { v.as_str() })
            .map(String::from);

        let entity_types = parse_uuid_array(&val, "entity_types")?;
        let seed_entities = parse_uuid_array(&val, "seed_entities")?;
        let introduced_entities = parse_uuid_array(&val, "introduced_entities")?;

        // Derive timestamp from the key — we don't store it separately
        let timestamp = Utc::now();

        Ok(SessionRecord {
            id: *session_id,
            slug,
            description,
            entity_types,
            seed_entities,
            introduced_entities,
            timestamp,
        })
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

fn parse_uuid_array(val: &serde_json::Value, field: &str) -> Result<Vec<Uuid>, LogError> {
    val.get(field)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().and_then(|s| Uuid::parse_str(s).ok()))
                .collect()
        })
        .ok_or_else(|| LogError::Storage(format!("missing {field}")))
}

fn encode_slug_key(slug: &str) -> Vec<u8> {
    slug.as_bytes().to_vec()
}

storage_key! {
    pub(crate) struct SessionKey(24) {
        session_id: Uuid,
        timestamp_us: i64,
    }
    prefixes {
        prefix_session(session_id: Uuid) -> 16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocksdb::{ColumnFamilyDescriptor, Options, DB};

    const MAIN_CF: &str = "session_main";
    const SLUG_CF: &str = "session_slug";

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

    fn record(id: Uuid, slug: &str) -> SessionRecord {
        SessionRecord {
            id,
            slug: slug.into(),
            description: None,
            entity_types: vec![],
            seed_entities: vec![],
            introduced_entities: vec![],
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn put_and_get_latest() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let io = SessionIo::new(Arc::clone(&db), MAIN_CF, SLUG_CF);

        let id = Uuid::now_v7();
        let et1 = Uuid::now_v7();
        let e1 = Uuid::now_v7();
        let mut rec = record(id, "my-session");
        rec.entity_types = vec![et1];
        rec.seed_entities = vec![e1];
        io.put_with_index(&rec).unwrap();

        let latest = io.get_latest(&id).unwrap().unwrap();
        assert_eq!(latest.id, id);
        assert_eq!(latest.slug, "my-session");
        assert_eq!(latest.entity_types, vec![et1]);
        assert_eq!(latest.seed_entities, vec![e1]);
        assert!(latest.introduced_entities.is_empty());
    }

    #[test]
    fn slug_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let io = SessionIo::new(Arc::clone(&db), MAIN_CF, SLUG_CF);

        let id = Uuid::now_v7();
        io.put_with_index(&record(id, "test")).unwrap();

        assert_eq!(io.get_uuid_by_slug("test").unwrap(), Some(id));
        assert!(io.get_uuid_by_slug("nope").unwrap().is_none());
    }

    #[test]
    fn update_preserves_latest() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let io = SessionIo::new(Arc::clone(&db), MAIN_CF, SLUG_CF);

        let id = Uuid::now_v7();
        io.put_with_index(&record(id, "evolving")).unwrap();

        std::thread::sleep(std::time::Duration::from_millis(1));
        let mut updated = record(id, "evolving");
        updated.introduced_entities = vec![Uuid::now_v7()];
        io.put(&updated).unwrap();

        let latest = io.get_latest(&id).unwrap().unwrap();
        assert_eq!(latest.introduced_entities.len(), 1);
    }

    #[test]
    fn scan_all() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let io = SessionIo::new(Arc::clone(&db), MAIN_CF, SLUG_CF);

        io.put_with_index(&record(Uuid::now_v7(), "a")).unwrap();
        io.put_with_index(&record(Uuid::now_v7(), "b")).unwrap();

        let all = io.scan_all_latest().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn key_roundtrip() {
        let key = SessionKey {
            session_id: Uuid::now_v7(),
            timestamp_us: 1_700_000_000_000_000,
        };
        let encoded = key.encode();
        assert_eq!(encoded.len(), SessionKey::SIZE);
        let decoded = SessionKey::decode(&encoded).unwrap();
        assert_eq!(decoded, key);
    }
}
