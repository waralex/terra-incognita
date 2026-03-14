//! Entity change — provenance record for a batch of assertions on one entity.
//!
//! Key: `change_id(16)` = 16 bytes.
//! Value: JSON with entity_id, tx_id, reasoning.
//! Append-only, global (not branch-scoped).
//!
//! Multiple assertions (different properties) reference the same change_id,
//! sharing one reasoning for the whole batch.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::io::{DbItem, DbError};
use crate::io::storage_key::storage_key;
use crate::io::storage_value::StorageValue;

const CF_ENTITY_CHANGES: &str = "entity_changes";

storage_key! {
    pub struct EntityChangeKey {
        change_id: Uuid,
    }
}

/// Entity change value — reasoning for a batch of property assertions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityChangeValue {
    pub entity_id: Uuid,
    pub tx_id: Uuid,
    pub reasoning: serde_json::Value,
}

impl StorageValue for EntityChangeValue {
    fn encode(&self) -> Result<Vec<u8>, DbError> {
        serde_json::to_vec(self).map_err(|e| DbError::Storage(e.to_string()))
    }

    fn decode(bytes: &[u8]) -> Result<Self, DbError> {
        serde_json::from_slice(bytes).map_err(|e| DbError::Storage(e.to_string()))
    }
}

/// Entity change entry = key + value.
#[derive(Debug, Clone)]
pub struct EntityChangeEntry {
    pub key: EntityChangeKey,
    pub value: EntityChangeValue,
}

impl DbItem for EntityChangeEntry {
    type Key = EntityChangeKey;
    type Value = EntityChangeValue;

    fn cf() -> &'static str {
        CF_ENTITY_CHANGES
    }

    fn key(&self) -> &EntityChangeKey {
        &self.key
    }

    fn value(&self) -> &EntityChangeValue {
        &self.value
    }

    fn from_parts(key: EntityChangeKey, value: EntityChangeValue) -> Self {
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
                reasoning: serde_json::json!("census data"),
            },
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<EntityChangeEntry>(&entry.key).unwrap().unwrap();
        assert_eq!(found.key.change_id, entry.key.change_id);
        assert_eq!(found.value.reasoning, serde_json::json!("census data"));
    }
}
