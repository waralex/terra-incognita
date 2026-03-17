//! Transaction log — denormalized index for quick transaction detail retrieval.
//!
//! Key: `tx_id(16)` = 16 bytes (global, not branch-scoped — UUID v7 is unique).
//! Value: JSON with branch slug, created/updated/touched/deleted entity info.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::io::{DbItem, DbError};
use crate::io::storage_key::storage_key;
use crate::io::storage_value::StorageValue;

const CF_TRANSACTION_LOG: &str = "transaction_log";

storage_key! {
    pub struct TransactionLogKey {
        tx_id: Uuid,
    }
}

/// A single entity change within a transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeItem {
    /// Entity slug.
    pub entity: String,
    /// Points to EntityChangeEntry for reasoning/meta.
    pub change_id: Uuid,
    /// Property slugs changed in this transaction.
    pub properties: Vec<String>,
}

/// Denormalized transaction summary — what happened in a single transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionLogValue {
    /// Branch slug where the transaction was committed.
    pub branch: String,
    /// Entities created in this transaction.
    pub created: Vec<ChangeItem>,
    /// Entities updated in this transaction.
    pub updated: Vec<ChangeItem>,
    /// Entity slugs explicitly touched (not from create/update).
    pub touched: Vec<String>,
    /// Entity slugs deleted in this transaction.
    pub deleted: Vec<String>,
}

impl StorageValue for TransactionLogValue {
    fn encode(&self) -> Result<Vec<u8>, DbError> {
        serde_json::to_vec(self).map_err(|e| DbError::Storage(e.to_string()))
    }

    fn decode(bytes: &[u8]) -> Result<Self, DbError> {
        serde_json::from_slice(bytes).map_err(|e| DbError::Storage(e.to_string()))
    }
}

/// Transaction log entry = key + value.
#[derive(Debug, Clone)]
pub struct TransactionLogEntry {
    pub key: TransactionLogKey,
    pub value: TransactionLogValue,
}

impl DbItem for TransactionLogEntry {
    type Key = TransactionLogKey;
    type Value = TransactionLogValue;

    fn cf() -> &'static str {
        CF_TRANSACTION_LOG
    }

    fn key(&self) -> &TransactionLogKey {
        &self.key
    }

    fn value(&self) -> &TransactionLogValue {
        &self.value
    }

    fn from_parts(key: TransactionLogKey, value: TransactionLogValue) -> Self {
        Self { key, value }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::TerraDb;

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = TerraDb::builder(dir.path())
            .with::<TransactionLogEntry>()
            .open()
            .unwrap();

        let tx_id = Uuid::now_v7();
        let entry = TransactionLogEntry {
            key: TransactionLogKey { tx_id },
            value: TransactionLogValue {
                branch: "main".into(),
                created: vec![ChangeItem {
                    entity: "alice".into(),
                    change_id: Uuid::now_v7(),
                    properties: vec!["age".into(), "name".into()],
                }],
                updated: vec![ChangeItem {
                    entity: "bob".into(),
                    change_id: Uuid::now_v7(),
                    properties: vec!["status".into()],
                }],
                touched: vec!["server".into()],
                deleted: vec!["old-item".into()],
            },
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<TransactionLogEntry>(&entry.key).unwrap().unwrap();
        assert_eq!(found.key.tx_id, tx_id);
        assert_eq!(found.value.branch, "main");
        assert_eq!(found.value.created.len(), 1);
        assert_eq!(found.value.created[0].entity, "alice");
        assert_eq!(found.value.created[0].properties, vec!["age", "name"]);
        assert_eq!(found.value.updated.len(), 1);
        assert_eq!(found.value.updated[0].entity, "bob");
        assert_eq!(found.value.touched, vec!["server"]);
        assert_eq!(found.value.deleted, vec!["old-item"]);
    }
}
