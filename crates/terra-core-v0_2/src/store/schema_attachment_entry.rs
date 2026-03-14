//! Schema attachment entry — links a property to an entity type.
//!
//! Key: `branch_id(16) | type_id(16) | prop_id(16)` = 48 bytes.
//! Value: tx_id (16 bytes) — when the attachment was created.

use uuid::Uuid;

use crate::io::{DbItem, DbError};
use crate::io::storage_key::{storage_key, StorageKey};

const CF_SCHEMA_ATTACHMENTS: &str = "schema_attachments";

storage_key! {
    pub(crate) struct SchemaAttachmentKeyRaw(48) {
        branch_id: Uuid,
        type_id: Uuid,
        prop_id: Uuid,
    }
    prefixes {
        prefix_branch(branch_id: Uuid) -> 16,
        prefix_branch_type(branch_id: Uuid, type_id: Uuid) -> 32,
    }
}

/// Schema attachment key.
#[derive(Debug, Clone)]
pub struct SchemaAttachmentKey {
    pub branch_id: Uuid,
    pub type_id: Uuid,
    pub prop_id: Uuid,
}

/// Schema attachment value.
#[derive(Debug, Clone)]
pub struct SchemaAttachmentValue {
    pub tx_id: Uuid,
}

/// Schema attachment entry = key + value.
#[derive(Debug, Clone)]
pub struct SchemaAttachmentEntry {
    pub key: SchemaAttachmentKey,
    pub value: SchemaAttachmentValue,
}

impl DbItem for SchemaAttachmentEntry {
    fn cf() -> &'static str {
        CF_SCHEMA_ATTACHMENTS
    }

    fn encode_key(&self) -> Vec<u8> {
        let raw = SchemaAttachmentKeyRaw {
            branch_id: self.key.branch_id,
            type_id: self.key.type_id,
            prop_id: self.key.prop_id,
        };
        raw.encode()
    }

    fn encode_value(&self) -> Result<Vec<u8>, DbError> {
        Ok(self.value.tx_id.as_bytes().to_vec())
    }

    fn decode(key: &[u8], value: &[u8]) -> Result<Self, DbError> {
        let raw = SchemaAttachmentKeyRaw::decode(key)
            .map_err(|e| DbError::Storage(e.to_string()))?;
        let tx_id = Uuid::from_slice(value)
            .map_err(|e| DbError::Storage(e.to_string()))?;
        Ok(Self {
            key: SchemaAttachmentKey {
                branch_id: raw.branch_id,
                type_id: raw.type_id,
                prop_id: raw.prop_id,
            },
            value: SchemaAttachmentValue { tx_id },
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
            .with::<SchemaAttachmentEntry>()
            .open()
            .unwrap();

        let entry = SchemaAttachmentEntry {
            key: SchemaAttachmentKey {
                branch_id: Uuid::now_v7(),
                type_id: Uuid::now_v7(),
                prop_id: Uuid::now_v7(),
            },
            value: SchemaAttachmentValue {
                tx_id: Uuid::now_v7(),
            },
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<SchemaAttachmentEntry>(&entry.encode_key()).unwrap().unwrap();
        assert_eq!(found.key.type_id, entry.key.type_id);
        assert_eq!(found.value.tx_id, entry.value.tx_id);
    }
}
