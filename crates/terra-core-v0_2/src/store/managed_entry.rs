//! Managed type entry — generic versioned records (tasks, etc.).
//!
//! Key: `type_hash(16) | branch_id(16) | item_id(16) | tx_id(16)` = 64 bytes.
//! Value: JSON with slug, optional state, and dynamic fields.
//!
//! `type_hash` is a UUID derived from the managed type name (from DataSchema).
//! All managed types share a single CF.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::io::storage_key::{storage_key, StorageKey};
use crate::io::{DbError, DbItem};

const CF_MANAGED_MAIN: &str = "managed_main";

storage_key! {
    pub(crate) struct ManagedKeyRaw(64) {
        type_hash: Uuid,
        branch_id: Uuid,
        item_id: Uuid,
        tx_id: Uuid,
    }
    prefixes {
        prefix_type(type_hash: Uuid) -> 16,
        prefix_type_branch(type_hash: Uuid, branch_id: Uuid) -> 32,
        prefix_type_branch_item(type_hash: Uuid, branch_id: Uuid, item_id: Uuid) -> 48,
    }
}

/// Managed type key.
#[derive(Debug, Clone)]
pub struct ManagedKey {
    pub type_hash: Uuid,
    pub branch_id: Uuid,
    pub item_id: Uuid,
    pub tx_id: Uuid,
}

/// Managed type value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedValue {
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    pub fields: serde_json::Map<String, serde_json::Value>,
}

/// Managed type entry = key + value.
#[derive(Debug, Clone)]
pub struct ManagedEntry {
    pub key: ManagedKey,
    pub value: ManagedValue,
}

impl DbItem for ManagedEntry {
    fn cf() -> &'static str {
        CF_MANAGED_MAIN
    }

    fn encode_key(&self) -> Vec<u8> {
        let raw = ManagedKeyRaw {
            type_hash: self.key.type_hash,
            branch_id: self.key.branch_id,
            item_id: self.key.item_id,
            tx_id: self.key.tx_id,
        };
        raw.encode()
    }

    fn encode_value(&self) -> Result<Vec<u8>, DbError> {
        serde_json::to_vec(&self.value).map_err(|e| DbError::Storage(e.to_string()))
    }

    fn decode(key: &[u8], value: &[u8]) -> Result<Self, DbError> {
        let raw = ManagedKeyRaw::decode(key).map_err(|e| DbError::Storage(e.to_string()))?;
        let val: ManagedValue =
            serde_json::from_slice(value).map_err(|e| DbError::Storage(e.to_string()))?;
        Ok(Self {
            key: ManagedKey {
                type_hash: raw.type_hash,
                branch_id: raw.branch_id,
                item_id: raw.item_id,
                tx_id: raw.tx_id,
            },
            value: val,
        })
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

        let found = db
            .get::<ManagedEntry>(&entry.encode_key())
            .unwrap()
            .unwrap();
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

        let found = db
            .get::<ManagedEntry>(&entry.encode_key())
            .unwrap()
            .unwrap();
        assert_eq!(found.value.state, None);
    }
}
