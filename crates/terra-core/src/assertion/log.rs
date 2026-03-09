use std::sync::Arc;

use chrono::{DateTime, Utc};
use rocksdb::DB;
use serde::Serialize;
use uuid::Uuid;

use super::key::{storage_key, StorageKey};

/// A single entry in an append-only log.
#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    /// Unique ID of this log entry (UUIDv7).
    pub id: Uuid,
    /// When this entry was recorded.
    pub timestamp: DateTime<Utc>,
    /// The entity this entry refers to.
    pub entity_id: Uuid,
    /// Arbitrary JSON body — property assertions, context, etc.
    pub body: serde_json::Value,
}

/// Errors from log operations.
#[derive(Debug, thiserror::Error)]
pub enum LogError {
    /// Underlying RocksDB or serialization error.
    #[error("storage error: {0}")]
    Storage(String),
}

/// Append-only log backed by a RocksDB column family.
pub struct AppendLog {
    db: Arc<DB>,
    cf_name: &'static str,
}

impl AppendLog {
    pub(crate) fn new(db: Arc<DB>, cf_name: &'static str) -> Self {
        Self { db, cf_name }
    }

    /// Appends a single entry. Generates timestamp and entry ID automatically.
    pub fn append(
        &self,
        entity_id: Uuid,
        body: serde_json::Value,
    ) -> Result<LogEntry, LogError> {
        let now = Utc::now();
        let timestamp_us = now.timestamp_micros();
        let entry_id = Uuid::now_v7();

        let key = LogKey {
            branch_id: super::MAIN_BRANCH,
            timestamp_us,
            entry_id,
            entity_id,
        };
        let value_bytes =
            serde_json::to_vec(&body).map_err(|e| LogError::Storage(e.to_string()))?;

        let cf = self.cf()?;
        self.db
            .put_cf(&cf, &key.encode(), &value_bytes)
            .map_err(|e| LogError::Storage(e.to_string()))?;

        Ok(LogEntry {
            id: entry_id,
            timestamp: now,
            entity_id,
            body,
        })
    }

    /// Appends multiple entries atomically via WriteBatch.
    pub fn append_batch(
        &self,
        items: &[(Uuid, serde_json::Value)],
    ) -> Result<Vec<LogEntry>, LogError> {
        let cf = self.cf()?;
        let mut batch = rocksdb::WriteBatch::default();
        let mut results = Vec::with_capacity(items.len());

        for (entity_id, body) in items {
            let now = Utc::now();
            let timestamp_us = now.timestamp_micros();
            let entry_id = Uuid::now_v7();

            let key = LogKey {
                branch_id: super::MAIN_BRANCH,
                timestamp_us,
                entry_id,
                entity_id: *entity_id,
            };
            let value_bytes =
                serde_json::to_vec(body).map_err(|e| LogError::Storage(e.to_string()))?;

            batch.put_cf(&cf, &key.encode(), &value_bytes);

            results.push(LogEntry {
                id: entry_id,
                timestamp: now,
                entity_id: *entity_id,
                body: body.clone(),
            });
        }

        self.db
            .write(batch)
            .map_err(|e| LogError::Storage(e.to_string()))?;

        Ok(results)
    }

    /// Returns all entries in reverse chronological order.
    pub fn list(&self) -> Result<Vec<LogEntry>, LogError> {
        let cf = self.cf()?;
        let mut entries = Vec::new();
        let iter = self.db.iterator_cf(&cf, rocksdb::IteratorMode::End);

        for item in iter {
            let (raw_key, val) = item.map_err(|e| LogError::Storage(e.to_string()))?;
            let k = LogKey::decode(&raw_key)?;

            let body: serde_json::Value =
                serde_json::from_slice(&val).map_err(|e| LogError::Storage(e.to_string()))?;

            let timestamp = DateTime::from_timestamp_micros(k.timestamp_us)
                .ok_or_else(|| LogError::Storage("invalid timestamp".into()))?;

            entries.push(LogEntry {
                id: k.entry_id,
                timestamp,
                entity_id: k.entity_id,
                body,
            });
        }

        Ok(entries)
    }

    pub(crate) fn cf(&self) -> Result<&rocksdb::ColumnFamily, LogError> {
        self.db
            .cf_handle(self.cf_name)
            .ok_or_else(|| LogError::Storage(format!("missing column family: {}", self.cf_name)))
    }
}

storage_key! {
    pub(crate) struct LogKey(56) {
        branch_id: Uuid,
        timestamp_us: i64,
        entry_id: Uuid,
        entity_id: Uuid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocksdb::{ColumnFamilyDescriptor, Options, DB};

    const TEST_CF: &str = "test_log";

    fn open_db(dir: &tempfile::TempDir) -> Arc<DB> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        let cf = ColumnFamilyDescriptor::new(TEST_CF, Options::default());
        Arc::new(DB::open_cf_descriptors(&opts, dir.path(), vec![cf]).unwrap())
    }

    #[test]
    fn append_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let log = AppendLog::new(Arc::clone(&db), TEST_CF);

        let entity_id = Uuid::now_v7();
        let body = serde_json::json!({"name": "alpha", "score": 42});

        let entry = log.append(entity_id, body.clone()).unwrap();
        assert_eq!(entry.entity_id, entity_id);
        assert_eq!(entry.body, body);
        assert_eq!(entry.id.get_version(), Some(uuid::Version::SortRand));

        let entries = log.list().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, entry.id);
        assert_eq!(entries[0].entity_id, entity_id);
        assert_eq!(entries[0].body, body);
    }

    #[test]
    fn append_batch_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let log = AppendLog::new(Arc::clone(&db), TEST_CF);

        let items: Vec<(Uuid, serde_json::Value)> = vec![
            (Uuid::now_v7(), serde_json::json!({"name": "first"})),
            (Uuid::now_v7(), serde_json::json!({"name": "second"})),
            (Uuid::now_v7(), serde_json::json!({"name": "third"})),
        ];

        let results = log.append_batch(&items).unwrap();
        assert_eq!(results.len(), 3);

        let entries = log.list().unwrap();
        assert_eq!(entries.len(), 3);
        // Reverse chronological — last appended first
        assert_eq!(entries[0].body["name"], "third");
        assert_eq!(entries[2].body["name"], "first");
    }

    #[test]
    fn list_empty_log() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let log = AppendLog::new(Arc::clone(&db), TEST_CF);

        let entries = log.list().unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn entries_have_unique_ids() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let log = AppendLog::new(Arc::clone(&db), TEST_CF);

        let e1 = log.append(Uuid::now_v7(), serde_json::json!({})).unwrap();
        let e2 = log.append(Uuid::now_v7(), serde_json::json!({})).unwrap();

        assert_ne!(e1.id, e2.id);
    }

    #[test]
    fn key_encoding_roundtrip() {
        let key = LogKey {
            branch_id: Uuid::nil(),
            timestamp_us: 1_700_000_000_000_000,
            entry_id: Uuid::now_v7(),
            entity_id: Uuid::now_v7(),
        };
        let encoded = key.encode();
        assert_eq!(encoded.len(), LogKey::SIZE);

        let decoded = LogKey::decode(&encoded).unwrap();
        assert_eq!(decoded, key);
    }

    #[test]
    fn keys_sort_by_branch_then_timestamp() {
        let branch = Uuid::nil();
        let id = Uuid::now_v7();
        let k1 = LogKey { branch_id: branch, timestamp_us: 100, entry_id: id, entity_id: id };
        let k2 = LogKey { branch_id: branch, timestamp_us: 200, entry_id: id, entity_id: id };
        assert!(k1.encode() < k2.encode());
    }
}
