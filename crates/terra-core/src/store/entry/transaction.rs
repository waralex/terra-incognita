//! Transaction metadata entry.
//!
//! Key: `branch(16) | tx_id(16)` = 32 bytes.
//! Value: JSON with dynamic metadata fields (defined by DataSchema).
//! Timestamp is derived from tx_id (UUID v7) — not stored separately.

use serde::{Deserialize, Serialize};

use crate::io::{DbItem, DbError};
use crate::io::storage_key::storage_key;
use crate::io::storage_value::StorageValue;

const CF_TRANSACTIONS: &str = "transactions";

storage_key! {
    pub struct TransactionKey {
        branch: Slug,
        tx_id: Uuid,
    }
}

/// Transaction value — dynamic metadata fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionValue {
    pub meta: serde_json::Map<String, serde_json::Value>,
}

impl StorageValue for TransactionValue {
    fn encode(&self) -> Result<Vec<u8>, DbError> {
        serde_json::to_vec(self).map_err(|e| DbError::Storage(e.to_string()))
    }

    fn decode(bytes: &[u8]) -> Result<Self, DbError> {
        serde_json::from_slice(bytes).map_err(|e| DbError::Storage(e.to_string()))
    }
}

/// Transaction entry = key + value.
#[derive(Debug, Clone)]
pub struct TransactionEntry {
    pub key: TransactionKey,
    pub value: TransactionValue,
}

impl DbItem for TransactionEntry {
    type Key = TransactionKey;
    type Value = TransactionValue;

    fn cf() -> &'static str {
        CF_TRANSACTIONS
    }

    fn key(&self) -> &TransactionKey {
        &self.key
    }

    fn value(&self) -> &TransactionValue {
        &self.value
    }

    fn from_parts(key: TransactionKey, value: TransactionValue) -> Self {
        Self { key, value }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    use crate::io::slug::Slug;
    use crate::io::TerraDb;

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = TerraDb::builder(dir.path())
            .with::<TransactionEntry>()
            .open()
            .unwrap();

        let mut meta = serde_json::Map::new();
        meta.insert("reasoning".into(), serde_json::json!("test"));

        let entry = TransactionEntry {
            key: TransactionKey {
                branch: "main".parse::<Slug>().unwrap(),
                tx_id: Uuid::now_v7(),
            },
            value: TransactionValue { meta },
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<TransactionEntry>(&entry.key).unwrap().unwrap();
        assert_eq!(found.key.tx_id, entry.key.tx_id);
        assert_eq!(found.value.meta["reasoning"], "test");
    }
}
