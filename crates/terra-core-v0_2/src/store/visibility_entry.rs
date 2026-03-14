//! Visibility control — hide/unhide items per branch.
//!
//! Key: `branch_id(16) | tx_id(16) | item_kind(16) | item_id(16)` = 64 bytes.
//! Value: `1` = hidden, `0` = visible. Default (no record) = visible.
//!
//! `item_kind` is a UUID identifying the namespace (same as in slug index).
//! Reads walk the ancestry chain, latest tx_id wins.


use crate::io::{DbItem, DbError};
use crate::io::storage_key::storage_key;
use crate::io::storage_value::StorageValue;

const CF_VISIBILITY: &str = "visibility";

storage_key! {
    pub struct VisibilityKey {
        branch_id: Uuid,
        tx_id: Uuid,
        item_kind: Uuid,
        item_id: Uuid,
    }
}
// Known prefixes: BranchPrefix(16)

/// Visibility value.
#[derive(Debug, Clone)]
pub struct VisibilityValue {
    pub hidden: bool,
}

impl StorageValue for VisibilityValue {
    fn encode(&self) -> Result<Vec<u8>, DbError> {
        Ok(vec![if self.hidden { 1 } else { 0 }])
    }

    fn decode(bytes: &[u8]) -> Result<Self, DbError> {
        let hidden = bytes.first().copied().unwrap_or(0) == 1;
        Ok(Self { hidden })
    }
}

/// Visibility entry = key + value.
#[derive(Debug, Clone)]
pub struct VisibilityEntry {
    pub key: VisibilityKey,
    pub value: VisibilityValue,
}

impl DbItem for VisibilityEntry {
    type Key = VisibilityKey;
    type Value = VisibilityValue;

    fn cf() -> &'static str {
        CF_VISIBILITY
    }

    fn key(&self) -> &VisibilityKey {
        &self.key
    }

    fn value(&self) -> &VisibilityValue {
        &self.value
    }

    fn from_parts(key: VisibilityKey, value: VisibilityValue) -> Self {
        Self { key, value }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
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
                item_kind: KIND,
                item_id: Uuid::now_v7(),
            },
            value: VisibilityValue { hidden: true },
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<VisibilityEntry>(&entry.key).unwrap().unwrap();
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
                item_kind: KIND,
                item_id: Uuid::now_v7(),
            },
            value: VisibilityValue { hidden: false },
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<VisibilityEntry>(&entry.key).unwrap().unwrap();
        assert!(!found.value.hidden);
    }
}
