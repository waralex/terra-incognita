//! Entity record entry.
//!
//! Key: `branch_id(16) | entity_id(16) | tx_id(16)` = 48 bytes.
//! Value: JSON with slug, entity_type_id, description.
//! Versioned — each mutation writes a new record with a new tx_id.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::io::{DbItem, DbError};
use crate::io::storage_key::{storage_key, StorageKey};

const CF_ENTITY_MAIN: &str = "entity_main";

storage_key! {
    pub struct EntityKey(48) {
        branch_id: Uuid,
        entity_id: Uuid,
        tx_id: Uuid,
    }
    prefixes {
        prefix_branch(branch_id: Uuid) -> 16,
        prefix_branch_entity(branch_id: Uuid, entity_id: Uuid) -> 32,
    }
}

/// Entity value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityValue {
    pub slug: String,
    pub entity_type_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<serde_json::Value>,
}

/// Entity entry = key + value.
#[derive(Debug, Clone)]
pub struct EntityEntry {
    pub key: EntityKey,
    pub value: EntityValue,
}

impl DbItem for EntityEntry {
    fn cf() -> &'static str {
        CF_ENTITY_MAIN
    }

    fn encode_key(&self) -> Vec<u8> {
        self.key.encode()
    }

    fn encode_value(&self) -> Result<Vec<u8>, DbError> {
        serde_json::to_vec(&self.value).map_err(|e| DbError::Storage(e.to_string()))
    }

    fn decode(key: &[u8], value: &[u8]) -> Result<Self, DbError> {
        let k = EntityKey::decode(key)
            .map_err(|e| DbError::Storage(e.to_string()))?;
        let val: EntityValue =
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
            .with::<EntityEntry>()
            .open()
            .unwrap();

        let entry = EntityEntry {
            key: EntityKey {
                branch_id: Uuid::now_v7(),
                entity_id: Uuid::now_v7(),
                tx_id: Uuid::now_v7(),
            },
            value: EntityValue {
                slug: "test-entity".into(),
                entity_type_id: Uuid::now_v7(),
                description: Some(serde_json::json!("A test")),
            },
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<EntityEntry>(&entry.encode_key()).unwrap().unwrap();
        assert_eq!(found.key.entity_id, entry.key.entity_id);
        assert_eq!(found.value.slug, "test-entity");
    }
}
