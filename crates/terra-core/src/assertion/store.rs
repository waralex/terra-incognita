use std::path::Path;

use chrono::Utc;
use rocksdb::{ColumnFamilyDescriptor, Options, DB};
use uuid::Uuid;

use crate::assertion::{AssertionError, AssertionKind, LogEntry};
use crate::schema::slug::validate_slug;
use crate::schema::SchemaError;

const CF_ASSERTIONS: &str = "assertions";

pub struct AssertionStore {
    db: DB,
}

impl AssertionStore {
    pub fn open(path: &Path) -> Result<Self, AssertionError> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        let cf = ColumnFamilyDescriptor::new(CF_ASSERTIONS, Options::default());
        let db = DB::open_cf_descriptors(&opts, path, vec![cf])
            .map_err(|e| AssertionError::Storage(e.to_string()))?;

        Ok(Self { db })
    }

    pub fn create_entity(
        &self,
        name: &str,
        entity_type: Option<&str>,
        context: serde_json::Value,
    ) -> Result<LogEntry, AssertionError> {
        validate_slug(name).map_err(|e| match e {
            SchemaError::InvalidSlug(s) => AssertionError::InvalidName(s),
            _ => AssertionError::Storage(e.to_string()),
        })?;

        let now = Utc::now();
        let timestamp_us = now.timestamp_micros();
        let log_entry_id = Uuid::now_v7();
        let entity_id = Uuid::now_v7();

        let key = encode_key(timestamp_us, &log_entry_id, &entity_id, AssertionKind::Hypothesis);

        let value = serde_json::json!({
            "entity_type": entity_type,
            "name": name,
            "context": context,
        });
        let value_bytes = serde_json::to_vec(&value)
            .map_err(|e| AssertionError::Storage(e.to_string()))?;

        let cf = self
            .db
            .cf_handle(CF_ASSERTIONS)
            .ok_or_else(|| AssertionError::Storage("missing column family".into()))?;

        self.db
            .put_cf(&cf, &key, &value_bytes)
            .map_err(|e| AssertionError::Storage(e.to_string()))?;

        Ok(LogEntry {
            id: log_entry_id,
            timestamp: now,
            entity_id,
            entity_type: entity_type.map(String::from),
            name: name.to_string(),
            context,
        })
    }

    pub fn list_log(&self) -> Result<Vec<LogEntry>, AssertionError> {
        let cf = self
            .db
            .cf_handle(CF_ASSERTIONS)
            .ok_or_else(|| AssertionError::Storage("missing column family".into()))?;

        let mut entries = Vec::new();
        let iter = self.db.iterator_cf(&cf, rocksdb::IteratorMode::End);

        for item in iter {
            let (key, val) = item.map_err(|e| AssertionError::Storage(e.to_string()))?;
            let (timestamp_us, log_entry_id, entity_id) = decode_key(&key)?;

            let record: serde_json::Value = serde_json::from_slice(&val)
                .map_err(|e| AssertionError::Storage(e.to_string()))?;

            let timestamp = chrono::DateTime::from_timestamp_micros(timestamp_us)
                .ok_or_else(|| AssertionError::Storage("invalid timestamp".into()))?;

            entries.push(LogEntry {
                id: log_entry_id,
                timestamp,
                entity_id,
                entity_type: record.get("entity_type")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                name: record.get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                context: record.get("context")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
            });
        }

        Ok(entries)
    }
}

fn encode_key(timestamp_us: i64, log_entry_id: &Uuid, entity_id: &Uuid, kind: AssertionKind) -> [u8; 41] {
    let mut key = [0u8; 41];
    key[0..8].copy_from_slice(&timestamp_us.to_be_bytes());
    key[8..24].copy_from_slice(log_entry_id.as_bytes());
    key[24..40].copy_from_slice(entity_id.as_bytes());
    key[40] = kind.as_byte();
    key
}

fn decode_key(key: &[u8]) -> Result<(i64, Uuid, Uuid), AssertionError> {
    if key.len() < 40 {
        return Err(AssertionError::Storage("invalid key length".into()));
    }
    let timestamp_us = i64::from_be_bytes(
        key[0..8].try_into().map_err(|_| AssertionError::Storage("bad timestamp".into()))?
    );
    let log_entry_id = Uuid::from_slice(&key[8..24])
        .map_err(|e| AssertionError::Storage(e.to_string()))?;
    let entity_id = Uuid::from_slice(&key[24..40])
        .map_err(|e| AssertionError::Storage(e.to_string()))?;
    Ok((timestamp_us, log_entry_id, entity_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(dir: &tempfile::TempDir) -> AssertionStore {
        AssertionStore::open(dir.path()).unwrap()
    }

    #[test]
    fn create_entity_returns_log_entry() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(&dir);

        let entry = s
            .create_entity("1st-tank-brigade", Some("military-unit"), serde_json::json!({}))
            .unwrap();

        assert_eq!(entry.entity_type.as_deref(), Some("military-unit"));
        assert_eq!(entry.name, "1st-tank-brigade");
        assert_eq!(entry.id.get_version(), Some(uuid::Version::SortRand));
        assert_eq!(entry.entity_id.get_version(), Some(uuid::Version::SortRand));
    }

    #[test]
    fn create_entity_without_type() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(&dir);

        let entry = s
            .create_entity("2nd-brigade", None, serde_json::json!({}))
            .unwrap();

        assert!(entry.entity_type.is_none());
        assert_eq!(entry.name, "2nd-brigade");
    }

    #[test]
    fn create_entity_with_context() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(&dir);

        let ctx = serde_json::json!({"source": "manual entry"});
        let entry = s
            .create_entity("3rd-brigade", Some("military-unit"), ctx.clone())
            .unwrap();

        assert_eq!(entry.context, ctx);
    }

    #[test]
    fn reject_invalid_name() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(&dir);

        let err = s
            .create_entity("Invalid Name", Some("military-unit"), serde_json::json!({}))
            .unwrap_err();

        assert!(matches!(err, AssertionError::InvalidName(_)));
    }

    #[test]
    fn reject_empty_name() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(&dir);

        let err = s
            .create_entity("", Some("military-unit"), serde_json::json!({}))
            .unwrap_err();

        assert!(matches!(err, AssertionError::InvalidName(_)));
    }

    #[test]
    fn multiple_entities_get_unique_ids() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(&dir);

        let e1 = s.create_entity("alpha", Some("unit"), serde_json::json!({})).unwrap();
        let e2 = s.create_entity("bravo", Some("unit"), serde_json::json!({})).unwrap();

        assert_ne!(e1.id, e2.id);
        assert_ne!(e1.entity_id, e2.entity_id);
    }

    #[test]
    fn list_log_returns_entries_reverse_chronological() {
        let dir = tempfile::tempdir().unwrap();
        let s = store(&dir);

        s.create_entity("alpha", Some("unit"), serde_json::json!({})).unwrap();
        s.create_entity("bravo", None, serde_json::json!({})).unwrap();

        let log = s.list_log().unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].name, "bravo");
        assert_eq!(log[1].name, "alpha");
    }

    #[test]
    fn key_encoding_is_41_bytes() {
        let id1 = Uuid::now_v7();
        let id2 = Uuid::now_v7();
        let key = encode_key(1_000_000, &id1, &id2, AssertionKind::Hypothesis);
        assert_eq!(key.len(), 41);
    }

    #[test]
    fn key_timestamp_sorts_lexicographically() {
        let id = Uuid::now_v7();
        let k1 = encode_key(100, &id, &id, AssertionKind::Hypothesis);
        let k2 = encode_key(200, &id, &id, AssertionKind::Hypothesis);
        assert!(k1 < k2);
    }
}
