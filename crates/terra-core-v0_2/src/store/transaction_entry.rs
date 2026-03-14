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
    pub(crate) struct TransactionKey(32) {
        branch_id: Uuid,
        tx_id: Uuid,
    }
    prefixes {
        prefix_branch(branch_id: Uuid) -> 16,
    }
}

/// Transaction metadata — dynamic fields defined by project config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionEntry {
    pub id: Uuid,
    pub branch_id: Uuid,
    pub meta: serde_json::Map<String, serde_json::Value>,
    pub timestamp: DateTime<Utc>,
}

impl DbItem for TransactionEntry {
    fn cf() -> &'static str {
        CF_TRANSACTIONS
    }

    fn encode_key(&self) -> Vec<u8> {
        let key = TransactionKey {
            branch_id: self.branch_id,
            tx_id: self.id,
        };
        key.encode()
    }

    fn encode_value(&self) -> Result<Vec<u8>, DbError> {
        serde_json::to_vec(self).map_err(|e| DbError::Storage(e.to_string()))
    }

    fn decode(key: &[u8], value: &[u8]) -> Result<Self, DbError> {
        let _k = TransactionKey::decode(key)
            .map_err(|e| DbError::Storage(e.to_string()))?;
        serde_json::from_slice(value).map_err(|e| DbError::Storage(e.to_string()))
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
            id: Uuid::now_v7(),
            branch_id: Uuid::now_v7(),
            meta,
            timestamp: Utc::now(),
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<TransactionEntry>(&entry.encode_key()).unwrap().unwrap();
        assert_eq!(found.id, entry.id);
        assert_eq!(found.meta["reasoning"], "test");
    }
}
