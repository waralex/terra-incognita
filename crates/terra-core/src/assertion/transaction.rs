use std::sync::Arc;

use chrono::{DateTime, Utc};
use rocksdb::DB;
use serde::Serialize;
use uuid::Uuid;

use super::log::LogError;

/// A transaction groups related assertions about an entity.
///
/// Transaction-level reasoning captures *why* this batch of assertions was made
/// (e.g. "analyzed this area and decided to narrow hypotheses"), while each
/// individual assertion carries its own reasoning for the specific value.
#[derive(Debug, Clone, Serialize)]
pub struct Transaction {
    pub id: Uuid,
    pub entity_id: Uuid,
    pub reasoning: serde_json::Value,
    pub timestamp: DateTime<Utc>,
}

/// Read/write access to the transaction CF.
///
/// Key: `tx_id` (16 bytes). Value: JSON `{entity_id, reasoning, timestamp}`.
pub struct TransactionStore {
    db: Arc<DB>,
    cf_name: &'static str,
}

impl TransactionStore {
    pub(crate) fn new(db: Arc<DB>, cf_name: &'static str) -> Self {
        Self { db, cf_name }
    }

    /// Writes a transaction record.
    pub fn put(&self, tx: &Transaction) -> Result<(), LogError> {
        let cf = self.cf()?;
        let val = serde_json::json!({
            "entity_id": tx.entity_id,
            "reasoning": tx.reasoning,
            "timestamp": tx.timestamp.to_rfc3339(),
        });
        let val_bytes = serde_json::to_vec(&val)
            .map_err(|e| LogError::Storage(e.to_string()))?;

        self.db
            .put_cf(cf, tx.id.as_bytes(), &val_bytes)
            .map_err(|e| LogError::Storage(e.to_string()))
    }

    /// Writes a transaction record into an existing WriteBatch.
    pub(crate) fn put_to_batch(
        &self,
        batch: &mut rocksdb::WriteBatch,
        tx: &Transaction,
    ) -> Result<(), LogError> {
        let cf = self.cf()?;
        let val = serde_json::json!({
            "entity_id": tx.entity_id,
            "reasoning": tx.reasoning,
            "timestamp": tx.timestamp.to_rfc3339(),
        });
        let val_bytes = serde_json::to_vec(&val)
            .map_err(|e| LogError::Storage(e.to_string()))?;

        batch.put_cf(cf, tx.id.as_bytes(), &val_bytes);
        Ok(())
    }

    /// Reads a transaction by ID.
    pub fn get(&self, tx_id: &Uuid) -> Result<Option<Transaction>, LogError> {
        let cf = self.cf()?;
        match self.db.get_cf(cf, tx_id.as_bytes()) {
            Ok(Some(bytes)) => {
                let val: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|e| LogError::Storage(e.to_string()))?;

                let entity_id = val.get("entity_id")
                    .and_then(|v| v.as_str())
                    .and_then(|s| Uuid::parse_str(s).ok())
                    .ok_or_else(|| LogError::Storage("missing entity_id in transaction".into()))?;

                let reasoning = val.get("reasoning").cloned()
                    .unwrap_or(serde_json::Value::Null);

                let timestamp = val.get("timestamp")
                    .and_then(|v| v.as_str())
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&Utc))
                    .ok_or_else(|| LogError::Storage("missing timestamp in transaction".into()))?;

                Ok(Some(Transaction { id: *tx_id, entity_id, reasoning, timestamp }))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(LogError::Storage(e.to_string())),
        }
    }

    fn cf(&self) -> Result<&rocksdb::ColumnFamily, LogError> {
        self.db
            .cf_handle(self.cf_name)
            .ok_or_else(|| LogError::Storage(format!("missing column family: {}", self.cf_name)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocksdb::{ColumnFamilyDescriptor, Options, DB};

    const TEST_CF: &str = "test_transactions";

    fn open_db(dir: &tempfile::TempDir) -> Arc<DB> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);
        let cf = ColumnFamilyDescriptor::new(TEST_CF, Options::default());
        Arc::new(DB::open_cf_descriptors(&opts, dir.path(), vec![cf]).unwrap())
    }

    #[test]
    fn put_and_get() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let store = TransactionStore::new(Arc::clone(&db), TEST_CF);

        let tx = Transaction {
            id: Uuid::now_v7(),
            entity_id: Uuid::now_v7(),
            reasoning: serde_json::json!("narrowed hypothesis space based on spectral analysis"),
            timestamp: Utc::now(),
        };

        store.put(&tx).unwrap();

        let loaded = store.get(&tx.id).unwrap().unwrap();
        assert_eq!(loaded.id, tx.id);
        assert_eq!(loaded.entity_id, tx.entity_id);
        assert_eq!(loaded.reasoning, tx.reasoning);
    }

    #[test]
    fn get_nonexistent() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let store = TransactionStore::new(Arc::clone(&db), TEST_CF);

        assert!(store.get(&Uuid::now_v7()).unwrap().is_none());
    }

    #[test]
    fn null_reasoning() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let store = TransactionStore::new(Arc::clone(&db), TEST_CF);

        let tx = Transaction {
            id: Uuid::now_v7(),
            entity_id: Uuid::now_v7(),
            reasoning: serde_json::Value::Null,
            timestamp: Utc::now(),
        };

        store.put(&tx).unwrap();
        let loaded = store.get(&tx.id).unwrap().unwrap();
        assert_eq!(loaded.reasoning, serde_json::Value::Null);
    }
}
