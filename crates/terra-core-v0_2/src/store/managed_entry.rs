//! Managed type entry — generic versioned records (tasks, etc.).
//!
//! Key: `branch_id(16) | type_hash(16) | item_id(16) | tx_id(16)` = 64 bytes.
//! Value: JSON with slug, optional state, and dynamic fields.
//!
//! `type_hash` is a UUID derived from the managed type name (from DataSchema).
//! All managed types share a single CF.

use serde::{Deserialize, Serialize};

use crate::io::storage_key::storage_key;
use crate::io::storage_value::StorageValue;
use crate::io::{DbError, DbItem};

const CF_MANAGED_MAIN: &str = "managed_main";

storage_key! {
    pub struct ManagedKey(64) {
        branch_id: Uuid,
        type_hash: Uuid,
        item_id: Uuid,
        tx_id: Uuid,
    }
    prefixes {
        prefix_branch(branch_id: Uuid) -> 16,
        prefix_branch_type(branch_id: Uuid, type_hash: Uuid) -> 32,
        prefix_branch_type_item(branch_id: Uuid, type_hash: Uuid, item_id: Uuid) -> 48,
    }
}

/// Managed type value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedValue {
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    pub fields: serde_json::Map<String, serde_json::Value>,
}

impl StorageValue for ManagedValue {
    fn encode(&self) -> Result<Vec<u8>, DbError> {
        serde_json::to_vec(self).map_err(|e| DbError::Storage(e.to_string()))
    }

    fn decode(bytes: &[u8]) -> Result<Self, DbError> {
        serde_json::from_slice(bytes).map_err(|e| DbError::Storage(e.to_string()))
    }
}

/// Managed type entry = key + value.
#[derive(Debug, Clone)]
pub struct ManagedEntry {
    pub key: ManagedKey,
    pub value: ManagedValue,
}

impl DbItem for ManagedEntry {
    type Key = ManagedKey;
    type Value = ManagedValue;

    fn cf() -> &'static str {
        CF_MANAGED_MAIN
    }

    fn key(&self) -> &ManagedKey {
        &self.key
    }

    fn value(&self) -> &ManagedValue {
        &self.value
    }

    fn from_parts(key: ManagedKey, value: ManagedValue) -> Self {
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
            .with::<ManagedEntry>()
            .open()
            .unwrap();

        let mut fields = serde_json::Map::new();
        fields.insert("goal".into(), serde_json::json!("explore orders table"));

        let entry = ManagedEntry {
            key: ManagedKey {
                type_hash: Uuid::from_u128(0xAAAA),
                branch_id: Uuid::now_v7(),
                item_id: Uuid::now_v7(),
                tx_id: Uuid::now_v7(),
            },
            value: ManagedValue {
                slug: "explore-orders".into(),
                state: Some("open".into()),
                fields,
            },
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<ManagedEntry>(&entry.key).unwrap().unwrap();
        assert_eq!(found.value.slug, "explore-orders");
        assert_eq!(found.value.state, Some("open".into()));
        assert_eq!(found.value.fields["goal"], "explore orders table");
    }

    #[test]
    fn without_lifecycle() {
        let dir = tempfile::tempdir().unwrap();
        let db = TerraDb::builder(dir.path())
            .with::<ManagedEntry>()
            .open()
            .unwrap();

        let entry = ManagedEntry {
            key: ManagedKey {
                type_hash: Uuid::from_u128(0xBBBB),
                branch_id: Uuid::now_v7(),
                item_id: Uuid::now_v7(),
                tx_id: Uuid::now_v7(),
            },
            value: ManagedValue {
                slug: "my-note".into(),
                state: None,
                fields: serde_json::Map::new(),
            },
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<ManagedEntry>(&entry.key).unwrap().unwrap();
        assert_eq!(found.value.state, None);
    }
}
