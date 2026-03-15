//! Entity record entry.
//!
//! Key: `hash(branch)(16) | hash(entity)(16) | tx_id(16)` = 48 bytes + slug suffixes.
//! Value: JSON with slug and description.
//! Versioned — each mutation writes a new record with a new tx_id.

use serde::{Deserialize, Serialize};

use crate::io::{DbItem, DbError};
use crate::io::storage_value::StorageValue;
use crate::store::versioned_key::versioned_key;

const CF_ENTITY_MAIN: &str = "entity_main";

versioned_key! {
    pub struct EntityKey {
        entity: Slug,
    }
}
// Known prefixes: BranchPrefix(16), BranchEntityPrefix(32)

/// Entity value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityValue {
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<serde_json::Value>,
}

impl StorageValue for EntityValue {
    fn encode(&self) -> Result<Vec<u8>, DbError> {
        serde_json::to_vec(self).map_err(|e| DbError::Storage(e.to_string()))
    }

    fn decode(bytes: &[u8]) -> Result<Self, DbError> {
        serde_json::from_slice(bytes).map_err(|e| DbError::Storage(e.to_string()))
    }
}

/// Entity entry = key + value.
#[derive(Debug, Clone)]
pub struct EntityEntry {
    pub key: EntityKey,
    pub value: EntityValue,
}

impl DbItem for EntityEntry {
    type Key = EntityKey;
    type Value = EntityValue;

    fn cf() -> &'static str {
        CF_ENTITY_MAIN
    }

    fn key(&self) -> &EntityKey {
        &self.key
    }

    fn value(&self) -> &EntityValue {
        &self.value
    }

    fn from_parts(key: EntityKey, value: EntityValue) -> Self {
        Self { key, value }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    use crate::io::TerraDb;
    use crate::io::slug::Slug;

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = TerraDb::builder(dir.path())
            .with::<EntityEntry>()
            .open()
            .unwrap();

        let entry = EntityEntry {
            key: EntityKey {
                branch: "main".parse::<Slug>().unwrap(),
                entity: "test-entity".parse::<Slug>().unwrap(),
                tx_id: Uuid::now_v7(),
            },
            value: EntityValue {
                slug: "test-entity".into(),
                description: Some(serde_json::json!("A test")),
            },
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<EntityEntry>(&entry.key).unwrap().unwrap();
        assert_eq!(found.key.entity, entry.key.entity);
        assert_eq!(found.value.slug, "test-entity");
    }
}
