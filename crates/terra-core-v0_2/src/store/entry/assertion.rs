//! Assertion entry — a single property value claim.
//!
//! Key: `branch(16) | prop(16) | tx_id(16) | change_id(16) | entity(16)` = 80 bytes fixed + slug suffixes.
//! Value: JSON (arbitrary property value).
//!
//! No fact/hypothesis distinction in v0.2 — all assertions are equal.

use crate::io::{DbItem, DbError};
use crate::io::storage_key::storage_key;
use crate::io::storage_value::StorageValue;


const CF_ASSERTIONS: &str = "assertions";

storage_key! {
    pub struct AssertionKey {
        branch: Slug,
        prop: Slug,
        tx_id: Uuid,
        change_id: Uuid,
        entity: Slug,
    }
}
// Known prefixes: BranchPrefix(16), BranchPropPrefix(32)

/// Assertion value.
#[derive(Debug, Clone)]
pub struct AssertionValue {
    pub value: serde_json::Value,
}

impl StorageValue for AssertionValue {
    fn encode(&self) -> Result<Vec<u8>, DbError> {
        serde_json::to_vec(&self.value).map_err(|e| DbError::Storage(e.to_string()))
    }

    fn decode(bytes: &[u8]) -> Result<Self, DbError> {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|e| DbError::Storage(e.to_string()))?;
        Ok(Self { value })
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
    use uuid::Uuid;
    use crate::io::TerraDb;

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = TerraDb::builder(dir.path())
            .with::<AssertionEntry>()
            .open()
            .unwrap();

        let entry = AssertionEntry {
            key: AssertionKey {
                branch: "main".parse::<crate::io::slug::Slug>().unwrap(),
                prop: "location".parse::<crate::io::slug::Slug>().unwrap(),
                tx_id: Uuid::now_v7(),
                change_id: Uuid::now_v7(),
                entity: "london".parse::<crate::io::slug::Slug>().unwrap(),
            },
            value: AssertionValue {
                value: serde_json::json!({"name": "London"}),
            },
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<AssertionEntry>(&entry.key).unwrap().unwrap();
        assert_eq!(found.key.entity, entry.key.entity);
        assert_eq!(found.value.value, serde_json::json!({"name": "London"}));
    }
}
