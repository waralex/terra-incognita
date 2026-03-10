use std::sync::Arc;

use rocksdb::DB;
use serde::Serialize;
use uuid::Uuid;

use super::key::{storage_key, StorageKey};

/// A single entry in an append-only log.
#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    /// Unique ID of this log entry (UUIDv7).
    pub id: Uuid,
    /// The entity this entry refers to.
    pub entity_id: Uuid,
    /// Transaction that produced this entry.
    pub tx_id: Uuid,
    /// Property assertions: property_id → typed value.
    pub properties: serde_json::Value,
    /// Why this assertion was made — free-form JSON (string, structured chain, references, etc.).
    pub reasoning: serde_json::Value,
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

    /// Appends a single entry with a given tx_id.
    pub fn append(
        &self,
        tx_id: Uuid,
        entity_id: Uuid,
        properties: serde_json::Value,
        reasoning: serde_json::Value,
    ) -> Result<LogEntry, LogError> {
        let entry_id = Uuid::now_v7();

        let key = LogKey {
            branch_id: super::MAIN_BRANCH,
            tx_id,
            entry_id,
            entity_id,
        };
        let stored = serde_json::json!({
            "properties": properties,
            "reasoning": reasoning,
        });
        let value_bytes =
            serde_json::to_vec(&stored).map_err(|e| LogError::Storage(e.to_string()))?;

        let cf = self.cf()?;
        self.db
            .put_cf(&cf, &key.encode(), &value_bytes)
            .map_err(|e| LogError::Storage(e.to_string()))?;

        Ok(LogEntry {
            id: entry_id,
            entity_id,
            tx_id,
            properties,
            reasoning,
        })
    }

    /// Appends multiple entries atomically via WriteBatch.
    pub fn append_batch(
        &self,
        tx_id: Uuid,
        items: &[(Uuid, serde_json::Value, serde_json::Value)],
    ) -> Result<Vec<LogEntry>, LogError> {
        let cf = self.cf()?;
        let mut batch = rocksdb::WriteBatch::default();
        let mut results = Vec::with_capacity(items.len());

        for (entity_id, properties, reasoning) in items {
            let entry_id = Uuid::now_v7();

            let key = LogKey {
                branch_id: super::MAIN_BRANCH,
                tx_id,
                entry_id,
                entity_id: *entity_id,
            };
            let stored = serde_json::json!({
                "properties": properties,
                "reasoning": reasoning,
            });
            let value_bytes =
                serde_json::to_vec(&stored).map_err(|e| LogError::Storage(e.to_string()))?;

            batch.put_cf(&cf, &key.encode(), &value_bytes);

            results.push(LogEntry {
                id: entry_id,
                entity_id: *entity_id,
                tx_id,
                properties: properties.clone(),
                reasoning: reasoning.clone(),
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

            let stored: serde_json::Value =
                serde_json::from_slice(&val).map_err(|e| LogError::Storage(e.to_string()))?;

            let properties = stored.get("properties").cloned().unwrap_or(serde_json::Value::Null);
            let reasoning = stored.get("reasoning").cloned().unwrap_or(serde_json::Value::Null);

            entries.push(LogEntry {
                id: k.entry_id,
                entity_id: k.entity_id,
                tx_id: k.tx_id,
                properties,
                reasoning,
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
    pub(crate) struct LogKey(64) {
        branch_id: Uuid,
        tx_id: Uuid,
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

        let tx_id = Uuid::now_v7();
        let entity_id = Uuid::now_v7();
        let props = serde_json::json!({"name": "alpha", "score": 42});
        let reasoning = serde_json::json!("initial observation");

        let entry = log.append(tx_id, entity_id, props.clone(), reasoning.clone()).unwrap();
        assert_eq!(entry.entity_id, entity_id);
        assert_eq!(entry.tx_id, tx_id);
        assert_eq!(entry.properties, props);
        assert_eq!(entry.reasoning, reasoning);
        assert_eq!(entry.id.get_version(), Some(uuid::Version::SortRand));

        let entries = log.list().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, entry.id);
        assert_eq!(entries[0].entity_id, entity_id);
        assert_eq!(entries[0].tx_id, tx_id);
        assert_eq!(entries[0].properties, props);
        assert_eq!(entries[0].reasoning, reasoning);
    }

    #[test]
    fn append_batch_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let log = AppendLog::new(Arc::clone(&db), TEST_CF);

        let tx_id = Uuid::now_v7();
        let items: Vec<(Uuid, serde_json::Value, serde_json::Value)> = vec![
            (Uuid::now_v7(), serde_json::json!({"name": "first"}), serde_json::json!(null)),
            (Uuid::now_v7(), serde_json::json!({"name": "second"}), serde_json::json!(null)),
            (Uuid::now_v7(), serde_json::json!({"name": "third"}), serde_json::json!("batch reason")),
        ];

        let results = log.append_batch(tx_id, &items).unwrap();
        assert_eq!(results.len(), 3);

        let entries = log.list().unwrap();
        assert_eq!(entries.len(), 3);
        // Reverse chronological — last appended first
        assert_eq!(entries[0].properties["name"], "third");
        assert_eq!(entries[0].reasoning, serde_json::json!("batch reason"));
        assert_eq!(entries[2].properties["name"], "first");
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

        let tx_id = Uuid::now_v7();
        let e1 = log.append(tx_id, Uuid::now_v7(), serde_json::json!({}), serde_json::json!(null)).unwrap();
        let e2 = log.append(tx_id, Uuid::now_v7(), serde_json::json!({}), serde_json::json!(null)).unwrap();

        assert_ne!(e1.id, e2.id);
    }

    #[test]
    fn key_encoding_roundtrip() {
        let key = LogKey {
            branch_id: Uuid::nil(),
            tx_id: Uuid::now_v7(),
            entry_id: Uuid::now_v7(),
            entity_id: Uuid::now_v7(),
        };
        let encoded = key.encode();
        assert_eq!(encoded.len(), LogKey::SIZE);

        let decoded = LogKey::decode(&encoded).unwrap();
        assert_eq!(decoded, key);
    }

    #[test]
    fn keys_sort_by_branch_then_tx() {
        let branch = Uuid::nil();
        let id = Uuid::now_v7();
        let tx1 = Uuid::from_u128(100);
        let tx2 = Uuid::from_u128(200);
        let k1 = LogKey { branch_id: branch, tx_id: tx1, entry_id: id, entity_id: id };
        let k2 = LogKey { branch_id: branch, tx_id: tx2, entry_id: id, entity_id: id };
        assert!(k1.encode() < k2.encode());
    }
}
