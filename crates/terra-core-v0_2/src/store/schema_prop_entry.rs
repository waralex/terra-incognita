//! Schema property entry.
//!
//! Key: `branch_id(16) | prop_id(16)` = 32 bytes.
//! Value: JSON with slug, description.
//! No ValueType — all values are arbitrary JSON in v0.2.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::io::{DbItem, DbError};
use crate::io::storage_key::{storage_key, StorageKey};

const CF_SCHEMA_PROPS: &str = "schema_props";

storage_key! {
    pub(crate) struct SchemaPropKey(32) {
        branch_id: Uuid,
        prop_id: Uuid,
    }
    prefixes {
        prefix_branch(branch_id: Uuid) -> 16,
    }
}

/// A registered property in the schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaPropEntry {
    pub id: Uuid,
    pub branch_id: Uuid,
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<serde_json::Value>,
}

impl DbItem for SchemaPropEntry {
    fn cf() -> &'static str {
        CF_SCHEMA_PROPS
    }

    fn encode_key(&self) -> Vec<u8> {
        let key = SchemaPropKey {
            branch_id: self.branch_id,
            prop_id: self.id,
        };
        key.encode()
    }

    fn encode_value(&self) -> Result<Vec<u8>, DbError> {
        serde_json::to_vec(self).map_err(|e| DbError::Storage(e.to_string()))
    }

    fn decode(key: &[u8], value: &[u8]) -> Result<Self, DbError> {
        let _k = SchemaPropKey::decode(key)
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
            .with::<SchemaPropEntry>()
            .open()
            .unwrap();

        let entry = SchemaPropEntry {
            id: Uuid::now_v7(),
            branch_id: Uuid::now_v7(),
            slug: "population".into(),
            description: None,
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<SchemaPropEntry>(&entry.encode_key()).unwrap().unwrap();
        assert_eq!(found.slug, "population");
    }
}
