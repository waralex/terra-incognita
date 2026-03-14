//! Visibility control — hide/unhide items per branch.
//!
//! Key: `branch_id(16) | tx_id(16) | kind(16) | item_id(16)` = 64 bytes.
//! Value: `1` = hidden, `0` = visible. Default (no record) = visible.
//!
//! `kind` is a UUID identifying the namespace (same as in slug index).
//! Reads walk the ancestry chain, latest tx_id wins.

use uuid::Uuid;

use crate::io::{DbItem, DbError};
use crate::io::storage_key::{storage_key, StorageKey};

const CF_VISIBILITY: &str = "visibility";

storage_key! {
    pub(crate) struct VisibilityKeyRaw(64) {
        branch_id: Uuid,
        tx_id: Uuid,
        item_kind: Uuid,
        item_id: Uuid,
    }
    prefixes {
        prefix_branch(branch_id: Uuid) -> 16,
    }
}

/// Visibility key.
#[derive(Debug, Clone)]
pub struct VisibilityKey {
    pub branch_id: Uuid,
    pub tx_id: Uuid,
    pub kind: Uuid,
    pub item_id: Uuid,
}

/// Visibility value.
#[derive(Debug, Clone)]
pub struct VisibilityValue {
    pub hidden: bool,
}

/// Visibility entry = key + value.
#[derive(Debug, Clone)]
pub struct VisibilityEntry {
    pub key: VisibilityKey,
    pub value: VisibilityValue,
}

impl DbItem for VisibilityEntry {
    fn cf() -> &'static str {
        CF_VISIBILITY
    }

    fn encode_key(&self) -> Vec<u8> {
        let raw = VisibilityKeyRaw {
            branch_id: self.key.branch_id,
            tx_id: self.key.tx_id,
            item_kind: self.key.kind,
            item_id: self.key.item_id,
        };
        raw.encode()
    }

    fn encode_value(&self) -> Result<Vec<u8>, DbError> {
        Ok(vec![if self.value.hidden { 1 } else { 0 }])
    }

    fn decode(key: &[u8], value: &[u8]) -> Result<Self, DbError> {
        let raw = VisibilityKeyRaw::decode(key)
            .map_err(|e| DbError::Storage(e.to_string()))?;
        let hidden = value.first().copied().unwrap_or(0) == 1;
        Ok(Self {
            key: VisibilityKey {
                branch_id: raw.branch_id,
                tx_id: raw.tx_id,
                kind: raw.item_kind,
                item_id: raw.item_id,
            },
            value: VisibilityValue { hidden },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::TerraDb;

    fn open_db(dir: &tempfile::TempDir) -> TerraDb {
        TerraDb::builder(dir.path())
            .with::<VisibilityEntry>()
            .open()
            .unwrap()
    }

    const KIND: Uuid = Uuid::from_u128(0);

    #[test]
    fn roundtrip_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);

        let entry = VisibilityEntry {
            key: VisibilityKey {
                branch_id: Uuid::now_v7(),
                tx_id: Uuid::now_v7(),
                kind: KIND,
                item_id: Uuid::now_v7(),
            },
            value: VisibilityValue { hidden: true },
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<VisibilityEntry>(&entry.encode_key()).unwrap().unwrap();
        assert!(found.value.hidden);
        assert_eq!(found.key.item_id, entry.key.item_id);
    }

    #[test]
    fn roundtrip_visible() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);

        let entry = VisibilityEntry {
            key: VisibilityKey {
                branch_id: Uuid::now_v7(),
                tx_id: Uuid::now_v7(),
                kind: KIND,
                item_id: Uuid::now_v7(),
            },
            value: VisibilityValue { hidden: false },
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<VisibilityEntry>(&entry.encode_key()).unwrap().unwrap();
        assert!(!found.value.hidden);
    }
}
