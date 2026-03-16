//! Assertion entry — a single property value claim.
//!
//! Key: `branch(16) | entity(16) | prop(16) | tx_id(16)` = 64 bytes fixed + slug suffixes.
//! Value: JSON with change_id and the property value.
//!
//! No fact/hypothesis distinction in v0.2 — all assertions are equal.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::io::{DbItem, DbError};
use crate::io::storage_value::StorageValue;
use crate::store::versioned_key::versioned_key;

const CF_ASSERTIONS: &str = "assertions";

versioned_key! {
    pub struct AssertionKey {
        entity: Slug,
        prop: Slug,
    }
}

/// Assertion value — property value + provenance link.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssertionValue {
    pub change_id: Uuid,
    pub value: serde_json::Value,
    pub reasoning: String,
}

impl StorageValue for AssertionValue {
    fn encode(&self) -> Result<Vec<u8>, DbError> {
        serde_json::to_vec(self).map_err(|e| DbError::Storage(e.to_string()))
    }

    fn decode(bytes: &[u8]) -> Result<Self, DbError> {
        serde_json::from_slice(bytes).map_err(|e| DbError::Storage(e.to_string()))
    }
}

/// Assertion entry = key + value.
#[derive(Debug, Clone)]
pub struct AssertionEntry {
    pub key: AssertionKey,
    pub value: AssertionValue,
}

impl DbItem for AssertionEntry {
    type Key = AssertionKey;
    type Value = AssertionValue;

    fn cf() -> &'static str {
        CF_ASSERTIONS
    }

    fn key(&self) -> &AssertionKey {
        &self.key
    }

    fn value(&self) -> &AssertionValue {
        &self.value
    }

    fn from_parts(key: AssertionKey, value: AssertionValue) -> Self {
        Self { key, value }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::TerraDb;
    use crate::io::slug::Slug;

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = TerraDb::builder(dir.path())
            .with::<AssertionEntry>()
            .open()
            .unwrap();

        let entry = AssertionEntry {
            key: AssertionKey {
                branch: "main".parse::<Slug>().unwrap(),
                entity: "london".parse::<Slug>().unwrap(),
                prop: "location".parse::<Slug>().unwrap(),
                tx_id: Uuid::now_v7(),
            },
            value: AssertionValue {
                change_id: Uuid::now_v7(),
                value: serde_json::json!({"name": "London"}),
                reasoning: "geographic data".into(),
            },
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<AssertionEntry>(&entry.key).unwrap().unwrap();
        assert_eq!(found.key.entity, entry.key.entity);
        assert_eq!(found.key.prop, entry.key.prop);
        assert_eq!(found.value.change_id, entry.value.change_id);
        assert_eq!(found.value.value, serde_json::json!({"name": "London"}));
    }
}
