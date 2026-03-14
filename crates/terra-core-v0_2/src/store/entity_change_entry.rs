//! Entity change — provenance record for a batch of assertions on one entity.
//!
//! Key: `change_id(16)` = 16 bytes.
//! Value: JSON with entity_id, tx_id, properties, reasoning.
//! Append-only, global (not branch-scoped).
//!
//! Multiple assertions (different properties) reference the same change_id,
//! sharing one reasoning for the whole batch.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::io::{DbItem, DbError};

const CF_ENTITY_CHANGES: &str = "entity_changes";

/// Entity change key.
#[derive(Debug, Clone)]
pub struct EntityChangeKey {
    pub change_id: Uuid,
}

/// Entity change value — reasoning for a batch of property assertions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityChangeValue {
    pub entity_id: Uuid,
    pub tx_id: Uuid,
    pub properties: serde_json::Value,
    pub reasoning: serde_json::Value,
}

/// Entity change entry = key + value.
#[derive(Debug, Clone)]
pub struct EntityChangeEntry {
    pub key: EntityChangeKey,
    pub value: EntityChangeValue,
}

impl DbItem for EntityChangeEntry {
    fn cf() -> &'static str {
        CF_ENTITY_CHANGES
    }

    fn encode_key(&self) -> Vec<u8> {
        self.key.change_id.as_bytes().to_vec()
    }

    fn encode_value(&self) -> Result<Vec<u8>, DbError> {
        serde_json::to_vec(&self.value).map_err(|e| DbError::Storage(e.to_string()))
    }

    fn decode(key: &[u8], value: &[u8]) -> Result<Self, DbError> {
        let change_id = Uuid::from_slice(key)
            .map_err(|e| DbError::Storage(e.to_string()))?;
        let val: EntityChangeValue =
            serde_json::from_slice(value).map_err(|e| DbError::Storage(e.to_string()))?;
        Ok(Self {
            key: EntityChangeKey { change_id },
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
            .with::<EntityChangeEntry>()
            .open()
            .unwrap();

        let entry = EntityChangeEntry {
            key: EntityChangeKey {
                change_id: Uuid::now_v7(),
            },
            value: EntityChangeValue {
                entity_id: Uuid::now_v7(),
                tx_id: Uuid::now_v7(),
                properties: serde_json::json!({"population": 56000000}),
                reasoning: serde_json::json!("census data"),
            },
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<EntityChangeEntry>(&entry.encode_key()).unwrap().unwrap();
        assert_eq!(found.key.change_id, entry.key.change_id);
        assert_eq!(found.value.reasoning, serde_json::json!("census data"));
    }
}
