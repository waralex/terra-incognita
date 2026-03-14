//! Schema entity type entry.
//!
//! Key: `branch_id(16) | type_id(16) | tx_id(16)` = 48 bytes.
//! Value: JSON with slug, description.

use serde::{Deserialize, Serialize};

use crate::io::{DbItem, DbError};
use crate::io::storage_key::storage_key;
use crate::io::storage_value::StorageValue;

const CF_SCHEMA_TYPES: &str = "schema_types";

storage_key! {
    pub struct SchemaTypeKey(48) {
        branch_id: Uuid,
        type_id: Uuid,
        tx_id: Uuid,
    }
    prefixes {
        prefix_branch(branch_id: Uuid) -> 16,
        prefix_branch_type(branch_id: Uuid, type_id: Uuid) -> 32,
    }
}

/// Schema type value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaTypeValue {
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<serde_json::Value>,
}

impl StorageValue for SchemaTypeValue {
    fn encode(&self) -> Result<Vec<u8>, DbError> {
        serde_json::to_vec(self).map_err(|e| DbError::Storage(e.to_string()))
    }

    fn decode(bytes: &[u8]) -> Result<Self, DbError> {
        serde_json::from_slice(bytes).map_err(|e| DbError::Storage(e.to_string()))
    }
}

/// Schema type entry = key + value.
#[derive(Debug, Clone)]
pub struct SchemaTypeEntry {
    pub key: SchemaTypeKey,
    pub value: SchemaTypeValue,
}

impl DbItem for SchemaTypeEntry {
    type Key = SchemaTypeKey;
    type Value = SchemaTypeValue;

    fn cf() -> &'static str {
        CF_SCHEMA_TYPES
    }

    fn key(&self) -> &SchemaTypeKey {
        &self.key
    }

    fn value(&self) -> &SchemaTypeValue {
        &self.value
    }

    fn from_parts(key: SchemaTypeKey, value: SchemaTypeValue) -> Self {
        Self { key, value }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
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
                tx_id: Uuid::now_v7(),
            },
            value: SchemaTypeValue {
                slug: "person".into(),
                description: Some(serde_json::json!("A person entity")),
            },
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<SchemaTypeEntry>(&entry.key).unwrap().unwrap();
        assert_eq!(found.value.slug, "person");
    }
}
