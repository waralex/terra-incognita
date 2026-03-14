//! Schema property entry.
//!
//! Key: `branch_id(16) | entity_type_id(16) | prop_id(16) | tx_id(16)` = 64 bytes.
//! Value: JSON with slug, description.
//! No ValueType — all values are arbitrary JSON in v0.2.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::io::{DbItem, DbError};
use crate::io::storage_key::{storage_key, StorageKey};

const CF_SCHEMA_PROPS: &str = "schema_props";

storage_key! {
    pub struct SchemaPropKey(64) {
        branch_id: Uuid,
        entity_type_id: Uuid,
        prop_id: Uuid,
        tx_id: Uuid,
    }
    prefixes {
        prefix_branch(branch_id: Uuid) -> 16,
        prefix_branch_type(branch_id: Uuid, entity_type_id: Uuid) -> 32,
        prefix_branch_type_prop(branch_id: Uuid, entity_type_id: Uuid, prop_id: Uuid) -> 48,
    }
}

/// Schema property value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaPropValue {
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<serde_json::Value>,
}

/// Schema property entry = key + value.
#[derive(Debug, Clone)]
pub struct SchemaPropEntry {
    pub key: SchemaPropKey,
    pub value: SchemaPropValue,
}

impl DbItem for SchemaPropEntry {
    fn cf() -> &'static str {
        CF_SCHEMA_PROPS
    }

    fn encode_key(&self) -> Vec<u8> {
        self.key.encode()
    }

    fn encode_value(&self) -> Result<Vec<u8>, DbError> {
        serde_json::to_vec(&self.value).map_err(|e| DbError::Storage(e.to_string()))
    }

    fn decode(key: &[u8], value: &[u8]) -> Result<Self, DbError> {
        let k = SchemaPropKey::decode(key)
            .map_err(|e| DbError::Storage(e.to_string()))?;
        let val: SchemaPropValue =
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
            .with::<SchemaPropEntry>()
            .open()
            .unwrap();

        let entry = SchemaPropEntry {
            key: SchemaPropKey {
                branch_id: Uuid::now_v7(),
                entity_type_id: Uuid::now_v7(),
                prop_id: Uuid::now_v7(),
                tx_id: Uuid::now_v7(),
            },
            value: SchemaPropValue {
                slug: "population".into(),
                description: None,
            },
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<SchemaPropEntry>(&entry.encode_key()).unwrap().unwrap();
        assert_eq!(found.value.slug, "population");
        assert_eq!(found.key.entity_type_id, entry.key.entity_type_id);
    }
}
