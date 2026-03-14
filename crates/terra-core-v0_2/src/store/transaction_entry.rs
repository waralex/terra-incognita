//! Transaction metadata entry.
//!
//! Key: `branch_id(16) | tx_id(16)` = 32 bytes.
//! Value: JSON with dynamic metadata fields (defined by DataSchema).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::io::{DbItem, DbError};
use crate::io::storage_key::{storage_key, StorageKey};

const CF_TRANSACTIONS: &str = "transactions";

storage_key! {
    pub struct TransactionKey(32) {
        branch_id: Uuid,
        tx_id: Uuid,
    }
    prefixes {
        prefix_branch(branch_id: Uuid) -> 16,
    }
}

/// Transaction value — dynamic metadata fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionValue {
    pub meta: serde_json::Map<String, serde_json::Value>,
    pub timestamp: DateTime<Utc>,
}

/// Transaction entry = key + value.
#[derive(Debug, Clone)]
pub struct TransactionEntry {
    pub key: TransactionKey,
    pub value: TransactionValue,
}

impl DbItem for TransactionEntry {
    fn cf() -> &'static str {
        CF_TRANSACTIONS
    }

    fn encode_key(&self) -> Vec<u8> {
        self.key.encode()
    }

    fn encode_value(&self) -> Result<Vec<u8>, DbError> {
        serde_json::to_vec(&self.value).map_err(|e| DbError::Storage(e.to_string()))
    }

    fn decode(key: &[u8], value: &[u8]) -> Result<Self, DbError> {
        let k = TransactionKey::decode(key)
            .map_err(|e| DbError::Storage(e.to_string()))?;
        let val: TransactionValue =
            serde_json::from_slice(value).map_err(|e| DbError::Storage(e.to_string()))?;
        Ok(Self { key: k, value: val })
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
            .with::<TransactionEntry>()
            .open()
            .unwrap();

        let mut meta = serde_json::Map::new();
        meta.insert("reasoning".into(), serde_json::json!("test"));

        let entry = TransactionEntry {
            key: TransactionKey {
                branch_id: Uuid::now_v7(),
                tx_id: Uuid::now_v7(),
            },
            value: TransactionValue {
                meta,
                timestamp: Utc::now(),
            },
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<TransactionEntry>(&entry.encode_key()).unwrap().unwrap();
        assert_eq!(found.key.tx_id, entry.key.tx_id);
        assert_eq!(found.value.meta["reasoning"], "test");
    }
}
