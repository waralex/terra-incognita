//! Schema attachment entry — links a property to an entity type.
//!
//! Key: `branch_id(16) | type_id(16) | prop_id(16)` = 48 bytes.
//! Value: minimal marker (tx_id when attached).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::io::{DbItem, DbError};
use crate::io::storage_key::{storage_key, StorageKey};

const CF_SCHEMA_ATTACHMENTS: &str = "schema_attachments";

storage_key! {
    pub(crate) struct SchemaAttachmentKey(48) {
        branch_id: Uuid,
        type_id: Uuid,
        prop_id: Uuid,
    }
    prefixes {
        prefix_branch(branch_id: Uuid) -> 16,
        prefix_branch_type(branch_id: Uuid, type_id: Uuid) -> 32,
    }
}

/// A property-to-type attachment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaAttachmentEntry {
    pub branch_id: Uuid,
    pub type_id: Uuid,
    pub prop_id: Uuid,
    pub tx_id: Uuid,
}

impl DbItem for SchemaAttachmentEntry {
    fn cf() -> &'static str {
        CF_SCHEMA_ATTACHMENTS
    }

    fn encode_key(&self) -> Vec<u8> {
        let key = SchemaAttachmentKey {
            branch_id: self.branch_id,
            type_id: self.type_id,
            prop_id: self.prop_id,
        };
        key.encode()
    }

    fn encode_value(&self) -> Result<Vec<u8>, DbError> {
        Ok(self.tx_id.as_bytes().to_vec())
    }

    fn decode(key: &[u8], value: &[u8]) -> Result<Self, DbError> {
        let k = SchemaAttachmentKey::decode(key)
            .map_err(|e| DbError::Storage(e.to_string()))?;
        let tx_id = Uuid::from_slice(value)
            .map_err(|e| DbError::Storage(e.to_string()))?;
        Ok(Self {
            branch_id: k.branch_id,
            type_id: k.type_id,
            prop_id: k.prop_id,
            tx_id,
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
            branch_id: Uuid::now_v7(),
            type_id: Uuid::now_v7(),
            prop_id: Uuid::now_v7(),
            tx_id: Uuid::now_v7(),
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<SchemaAttachmentEntry>(&entry.encode_key()).unwrap().unwrap();
        assert_eq!(found.type_id, entry.type_id);
        assert_eq!(found.prop_id, entry.prop_id);
        assert_eq!(found.tx_id, entry.tx_id);
    }
}
