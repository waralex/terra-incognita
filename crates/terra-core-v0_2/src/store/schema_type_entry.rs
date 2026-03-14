//! Schema entity type entry.
//!
//! Key: `branch_id(16) | type_id(16)` = 32 bytes.
//! Value: JSON with slug, description.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::io::{DbItem, DbError};
use crate::io::storage_key::{storage_key, StorageKey};

const CF_SCHEMA_TYPES: &str = "schema_types";

storage_key! {
    pub struct SchemaTypeKey(32) {
        branch_id: Uuid,
        type_id: Uuid,
    }
    prefixes {
        prefix_branch(branch_id: Uuid) -> 16,
    }
}

/// Schema type value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaTypeValue {
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<serde_json::Value>,
}

/// Schema type entry = key + value.
#[derive(Debug, Clone)]
pub struct SchemaTypeEntry {
    pub key: SchemaTypeKey,
    pub value: SchemaTypeValue,
}

impl DbItem for SchemaTypeEntry {
    fn cf() -> &'static str {
        CF_SCHEMA_TYPES
    }

    fn encode_key(&self) -> Vec<u8> {
        self.key.encode()
    }

    fn encode_value(&self) -> Result<Vec<u8>, DbError> {
        serde_json::to_vec(&self.value).map_err(|e| DbError::Storage(e.to_string()))
    }

    fn decode(key: &[u8], value: &[u8]) -> Result<Self, DbError> {
        let k = SchemaTypeKey::decode(key)
            .map_err(|e| DbError::Storage(e.to_string()))?;
        let val: SchemaTypeValue =
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
            .with::<SchemaTypeEntry>()
            .open()
            .unwrap();

        let entry = SchemaTypeEntry {
            key: SchemaTypeKey {
                branch_id: Uuid::now_v7(),
                type_id: Uuid::now_v7(),
            },
            value: SchemaTypeValue {
                slug: "person".into(),
                description: Some(serde_json::json!("A person entity")),
            },
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<SchemaTypeEntry>(&entry.encode_key()).unwrap().unwrap();
        assert_eq!(found.value.slug, "person");
    }
}
