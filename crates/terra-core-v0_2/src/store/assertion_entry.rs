//! Assertion entry — a single property value claim.
//!
//! Key: `branch_id(16) | prop_id(16) | tx_id(16) | entry_id(16) | entity_id(16)` = 80 bytes.
//! Value: JSON (arbitrary property value).
//!
//! No fact/hypothesis distinction in v0.2 — all assertions are equal.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::io::{DbItem, DbError};
use crate::io::storage_key::{storage_key, StorageKey};

const CF_ASSERTIONS: &str = "assertions";

storage_key! {
    pub(crate) struct AssertionKey(80) {
        branch_id: Uuid,
        prop_id: Uuid,
        tx_id: Uuid,
        entry_id: Uuid,
        entity_id: Uuid,
    }
    prefixes {
        prefix_branch(branch_id: Uuid) -> 16,
        prefix_branch_prop(branch_id: Uuid, prop_id: Uuid) -> 32,
    }
}

/// A single assertion: one property value for one entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssertionEntry {
    pub branch_id: Uuid,
    pub prop_id: Uuid,
    pub tx_id: Uuid,
    pub entry_id: Uuid,
    pub entity_id: Uuid,
    pub value: serde_json::Value,
}

impl DbItem for AssertionEntry {
    fn cf() -> &'static str {
        CF_ASSERTIONS
    }

    fn encode_key(&self) -> Vec<u8> {
        let key = AssertionKey {
            branch_id: self.branch_id,
            prop_id: self.prop_id,
            tx_id: self.tx_id,
            entry_id: self.entry_id,
            entity_id: self.entity_id,
        };
        key.encode()
    }

    fn encode_value(&self) -> Result<Vec<u8>, DbError> {
        serde_json::to_vec(&self.value).map_err(|e| DbError::Storage(e.to_string()))
    }

    fn decode(key: &[u8], value: &[u8]) -> Result<Self, DbError> {
        let k = AssertionKey::decode(key)
            .map_err(|e| DbError::Storage(e.to_string()))?;
        let val: serde_json::Value =
            serde_json::from_slice(value).map_err(|e| DbError::Storage(e.to_string()))?;
        Ok(Self {
            branch_id: k.branch_id,
            prop_id: k.prop_id,
            tx_id: k.tx_id,
            entry_id: k.entry_id,
            entity_id: k.entity_id,
            value: val,
        })
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
            .with::<AssertionEntry>()
            .open()
            .unwrap();

        let entry = AssertionEntry {
            branch_id: Uuid::now_v7(),
            prop_id: Uuid::now_v7(),
            tx_id: Uuid::now_v7(),
            entry_id: Uuid::now_v7(),
            entity_id: Uuid::now_v7(),
            value: serde_json::json!({"name": "London"}),
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<AssertionEntry>(&entry.encode_key()).unwrap().unwrap();
        assert_eq!(found.entity_id, entry.entity_id);
        assert_eq!(found.value, serde_json::json!({"name": "London"}));
    }
}
