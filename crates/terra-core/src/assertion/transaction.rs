use std::sync::Arc;

use chrono::{DateTime, Utc};
use rocksdb::DB;
use serde::Serialize;
use uuid::Uuid;

use super::key::{storage_key, StorageKey};
use super::log::LogError;

storage_key! {
    pub(crate) struct TransactionKey(32) {
        branch_id: Uuid,
        tx_id: Uuid,
    }
    prefixes {
        prefix_branch(branch_id: Uuid) -> 16,
    }
}

/// A transaction groups related assertions.
///
/// Transaction-level reasoning captures *why* this batch of assertions was made
/// (e.g. "analyzed this area and decided to narrow hypotheses"), while each
/// individual assertion carries its own reasoning for the specific value.
#[derive(Debug, Clone, Serialize)]
pub struct Transaction {
    pub id: Uuid,
    pub branch_id: Uuid,
    pub reasoning: serde_json::Value,
    pub timestamp: DateTime<Utc>,
}

/// Read/write access to the transaction CF.
///
/// Key: `branch_id(16) | tx_id(16)` = 32 bytes. Value: JSON `{branch_id, reasoning, timestamp}`.
pub struct TransactionStore {
    db: Arc<DB>,
    cf_name: &'static str,
}

impl TransactionStore {
    pub(crate) fn new(db: Arc<DB>, cf_name: &'static str) -> Self {
        Self { db, cf_name }
    }

    fn serialize_tx(tx: &Transaction) -> serde_json::Value {
        serde_json::json!({
            "branch_id": tx.branch_id.to_string(),
            "reasoning": tx.reasoning,
            "timestamp": tx.timestamp.to_rfc3339(),
        })
    }

    /// Writes a transaction record.
    pub fn put(&self, tx: &Transaction) -> Result<(), LogError> {
        let cf = self.cf()?;
        let key = TransactionKey {
            branch_id: tx.branch_id,
            tx_id: tx.id,
        };
        let val_bytes = serde_json::to_vec(&Self::serialize_tx(tx))
            .map_err(|e| LogError::Storage(e.to_string()))?;

        self.db
            .put_cf(cf, key.encode(), &val_bytes)
            .map_err(|e| LogError::Storage(e.to_string()))
    }

    /// Writes a transaction record into an existing WriteBatch.
    pub(crate) fn put_to_batch(
        &self,
        batch: &mut rocksdb::WriteBatch,
        tx: &Transaction,
    ) -> Result<(), LogError> {
        let cf = self.cf()?;
        let key = TransactionKey {
            branch_id: tx.branch_id,
            tx_id: tx.id,
        };
        let val_bytes = serde_json::to_vec(&Self::serialize_tx(tx))
            .map_err(|e| LogError::Storage(e.to_string()))?;

        batch.put_cf(cf, key.encode(), &val_bytes);
        Ok(())
    }

    /// Reads a transaction by branch and ID.
    pub fn get(&self, branch_id: &Uuid, tx_id: &Uuid) -> Result<Option<Transaction>, LogError> {
        let cf = self.cf()?;
        let key = TransactionKey {
            branch_id: *branch_id,
            tx_id: *tx_id,
        };
        match self.db.get_cf(cf, key.encode()) {
            Ok(Some(bytes)) => {
                let val: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|e| LogError::Storage(e.to_string()))?;

                let reasoning = val.get("reasoning").cloned()
                    .unwrap_or(serde_json::Value::Null);

                let timestamp = val.get("timestamp")
                    .and_then(|v| v.as_str())
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&Utc))
                    .ok_or_else(|| LogError::Storage("missing timestamp in transaction".into()))?;

                Ok(Some(Transaction { id: *tx_id, branch_id: *branch_id, reasoning, timestamp }))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(LogError::Storage(e.to_string())),
        }
    }

    /// Lists all transactions for a given branch via prefix scan.
    pub fn list_by_branch(&self, branch_id: &Uuid) -> Result<Vec<Transaction>, LogError> {
        let cf = self.cf()?;
        let prefix = TransactionKey::prefix_branch(branch_id);
        let mut results = Vec::new();

        let iter = self.db.prefix_iterator_cf(cf, &prefix);
        for item in iter {
            let (raw_key, val) = item.map_err(|e| LogError::Storage(e.to_string()))?;
            if !raw_key.starts_with(&prefix) {
                break;
            }

            let k = TransactionKey::decode(&raw_key)?;

            let parsed: serde_json::Value = serde_json::from_slice(&val)
                .map_err(|e| LogError::Storage(e.to_string()))?;

            let reasoning = parsed.get("reasoning").cloned()
                .unwrap_or(serde_json::Value::Null);

            let timestamp = parsed.get("timestamp")
                .and_then(|v| v.as_str())
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&Utc))
                .ok_or_else(|| LogError::Storage("missing timestamp in transaction".into()))?;

            results.push(Transaction {
                id: k.tx_id,
                branch_id: k.branch_id,
                reasoning,
                timestamp,
            });
        }

        Ok(results)
    }

    /// Lists transactions for a branch where tx_id <= upper_bound.
    pub fn list_by_branch_at(
        &self,
        branch_id: &Uuid,
        upper_bound: &Uuid,
    ) -> Result<Vec<Transaction>, LogError> {
        let cf = self.cf()?;
        let prefix = TransactionKey::prefix_branch(branch_id);
        let bound = *upper_bound.as_bytes();
        let mut results = Vec::new();

        let iter = self.db.prefix_iterator_cf(cf, &prefix);
        for item in iter {
            let (raw_key, val) = item.map_err(|e| LogError::Storage(e.to_string()))?;
            if !raw_key.starts_with(&prefix) {
                break;
            }

            let k = TransactionKey::decode(&raw_key)?;
            if *k.tx_id.as_bytes() > bound {
                break; // keys sorted by tx_id within branch — no more matches
            }

            let parsed: serde_json::Value = serde_json::from_slice(&val)
                .map_err(|e| LogError::Storage(e.to_string()))?;

            let reasoning = parsed.get("reasoning").cloned()
                .unwrap_or(serde_json::Value::Null);

            let timestamp = parsed.get("timestamp")
                .and_then(|v| v.as_str())
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&Utc))
                .ok_or_else(|| LogError::Storage("missing timestamp in transaction".into()))?;

            results.push(Transaction {
                id: k.tx_id,
                branch_id: k.branch_id,
                reasoning,
                timestamp,
            });
        }

        Ok(results)
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
        opts.set_prefix_extractor(rocksdb::SliceTransform::create_fixed_prefix(16));
        let cf_opts = {
            let mut o = Options::default();
            o.set_prefix_extractor(rocksdb::SliceTransform::create_fixed_prefix(16));
            o
        };
        let cf = ColumnFamilyDescriptor::new(TEST_CF, cf_opts);
        Arc::new(DB::open_cf_descriptors(&opts, dir.path(), vec![cf]).unwrap())
    }

    #[test]
    fn put_and_get() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let store = TransactionStore::new(Arc::clone(&db), TEST_CF);

        let branch_id = Uuid::now_v7();
        let tx = Transaction {
            id: Uuid::now_v7(),
            branch_id,
            reasoning: serde_json::json!("narrowed hypothesis space based on spectral analysis"),
            timestamp: Utc::now(),
        };

        store.put(&tx).unwrap();

        let loaded = store.get(&branch_id, &tx.id).unwrap().unwrap();
        assert_eq!(loaded.id, tx.id);
        assert_eq!(loaded.branch_id, tx.branch_id);
        assert_eq!(loaded.reasoning, tx.reasoning);
    }

    #[test]
    fn get_nonexistent() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let store = TransactionStore::new(Arc::clone(&db), TEST_CF);

        assert!(store.get(&Uuid::now_v7(), &Uuid::now_v7()).unwrap().is_none());
    }

    #[test]
    fn null_reasoning() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let store = TransactionStore::new(Arc::clone(&db), TEST_CF);

        let branch_id = Uuid::now_v7();
        let tx = Transaction {
            id: Uuid::now_v7(),
            branch_id,
            reasoning: serde_json::Value::Null,
            timestamp: Utc::now(),
        };

        store.put(&tx).unwrap();
        let loaded = store.get(&branch_id, &tx.id).unwrap().unwrap();
        assert_eq!(loaded.reasoning, serde_json::Value::Null);
    }

    #[test]
    fn list_by_branch_returns_matching() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let store = TransactionStore::new(Arc::clone(&db), TEST_CF);

        let branch_a = Uuid::now_v7();
        let branch_b = Uuid::now_v7();

        let tx1 = Transaction {
            id: Uuid::now_v7(),
            branch_id: branch_a,
            reasoning: serde_json::json!("first"),
            timestamp: Utc::now(),
        };
        let tx2 = Transaction {
            id: Uuid::now_v7(),
            branch_id: branch_a,
            reasoning: serde_json::json!("second"),
            timestamp: Utc::now(),
        };
        let tx3 = Transaction {
            id: Uuid::now_v7(),
            branch_id: branch_b,
            reasoning: serde_json::json!("other branch"),
            timestamp: Utc::now(),
        };

        store.put(&tx1).unwrap();
        store.put(&tx2).unwrap();
        store.put(&tx3).unwrap();

        let results = store.list_by_branch(&branch_a).unwrap();
        assert_eq!(results.len(), 2);
        for r in &results {
            assert_eq!(r.branch_id, branch_a);
        }

        let results_b = store.list_by_branch(&branch_b).unwrap();
        assert_eq!(results_b.len(), 1);
        assert_eq!(results_b[0].branch_id, branch_b);
    }

    #[test]
    fn list_by_branch_empty() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);
        let store = TransactionStore::new(Arc::clone(&db), TEST_CF);

        let results = store.list_by_branch(&Uuid::now_v7()).unwrap();
        assert!(results.is_empty());
    }
}
