//! TransactionWriter — atomic mutation context created by a Branch.
//!
//! Generates a tx_id (UUID v7), holds a WriteBatch, and on commit
//! writes the transaction record alongside any accumulated operations.

use serde_json::{Map, Value};
use uuid::Uuid;

use crate::io::{DbError, WriteBatch};
use crate::io::slug::Slug;
use crate::store::transaction_entry::{TransactionEntry, TransactionKey, TransactionValue};

/// Atomic mutation context bound to a branch.
///
/// Created via [`Branch::transaction`]. On `commit()`, writes the
/// transaction record and all accumulated operations atomically.
pub struct TransactionWriter {
    branch_id: Slug,
    tx_id: Uuid,
    meta: Map<String, Value>,
    batch: WriteBatch,
}

impl TransactionWriter {
    pub(crate) fn new(
        branch_id: Slug,
        meta: Map<String, Value>,
        batch: WriteBatch,
    ) -> Self {
        Self {
            branch_id,
            tx_id: Uuid::now_v7(),
            meta,
            batch,
        }
    }

    /// Transaction UUID (v7, time-ordered).
    pub fn tx_id(&self) -> Uuid {
        self.tx_id
    }

    /// Branch this transaction belongs to.
    pub fn branch_id(&self) -> &Slug {
        &self.branch_id
    }

    /// Commit the transaction atomically.
    pub fn commit(mut self) -> Result<Uuid, DbError> {
        let tx_id = self.tx_id;

        let entry = TransactionEntry {
            key: TransactionKey {
                branch_id: self.branch_id,
                tx_id,
            },
            value: TransactionValue {
                meta: self.meta,
            },
        };

        self.batch.put(&entry)?;
        self.batch.commit()?;
        Ok(tx_id)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::config::ProjectConfig;
    use crate::store::branch::main_branch_slug;
    use crate::store::storage::Storage;
    use crate::store::transaction_entry::{TransactionEntry, TransactionKey};
    use crate::store::prefix::BranchPrefix;

    fn test_storage() -> (tempfile::TempDir, Storage) {
        let dir = tempfile::tempdir().unwrap();
        let config = Arc::new(ProjectConfig::builder()
            .data_dir("./data".into())
            .schema_path("./schema.yaml".into())
            .build());
        let storage = Storage::open(dir.path(), config).unwrap();
        (dir, storage)
    }

    #[test]
    fn commit_writes_transaction_record() {
        let (_dir, storage) = test_storage();
        let branch = storage.main_branch();

        let mut meta = serde_json::Map::new();
        meta.insert("reasoning".into(), serde_json::json!("test commit"));

        let tx = branch.transaction(meta).unwrap();
        let tx_id = tx.tx_id();
        tx.commit().unwrap();

        let key = TransactionKey { branch_id: main_branch_slug(), tx_id };
        let found = storage.db.get::<TransactionEntry>(&key).unwrap();
        assert!(found.is_some());
        let found = found.unwrap();
        assert_eq!(found.value.meta["reasoning"], "test commit");
    }

    #[test]
    fn tx_id_is_unique() {
        let (_dir, storage) = test_storage();
        let branch = storage.main_branch();

        let tx1 = branch.transaction(serde_json::Map::new()).unwrap();
        let tx2 = branch.transaction(serde_json::Map::new()).unwrap();
        assert_ne!(tx1.tx_id(), tx2.tx_id());

        tx1.commit().unwrap();
        tx2.commit().unwrap();
    }

    #[test]
    fn scan_transactions_on_branch() {
        let (_dir, storage) = test_storage();
        let branch = storage.main_branch();

        let mut meta = serde_json::Map::new();
        meta.insert("step".into(), serde_json::json!(1));
        branch.transaction(meta).unwrap().commit().unwrap();

        let mut meta = serde_json::Map::new();
        meta.insert("step".into(), serde_json::json!(2));
        branch.transaction(meta).unwrap().commit().unwrap();

        let prefix = BranchPrefix { branch_id: main_branch_slug() };
        let txs: Vec<TransactionEntry> = storage.db
            .scan::<TransactionEntry>(&prefix)
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();

        assert_eq!(txs.len(), 2);
        assert_eq!(txs[0].value.meta["step"], 1);
        assert_eq!(txs[1].value.meta["step"], 2);
    }
}
