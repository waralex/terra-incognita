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
    pub(crate) struct VisibilityKey(64) {
        branch_id: Uuid,
        tx_id: Uuid,
        item_kind: Uuid,
        item_id: Uuid,
    }
    prefixes {
        prefix_branch(branch_id: Uuid) -> 16,
    }
}

/// A visibility record ready for storage.
pub struct VisibilityEntry {
    pub branch_id: Uuid,
    pub tx_id: Uuid,
    pub kind: Uuid,
    pub item_id: Uuid,
    pub hidden: bool,
}

impl DbItem for VisibilityEntry {
    fn cf() -> &'static str {
        CF_VISIBILITY
    }

    fn encode_key(&self) -> Vec<u8> {
        let key = VisibilityKey {
            branch_id: self.branch_id,
            tx_id: self.tx_id,
            item_kind: self.kind,
            item_id: self.item_id,
        };
        key.encode()
    }

    fn encode_value(&self) -> Result<Vec<u8>, DbError> {
        Ok(vec![if self.hidden { 1 } else { 0 }])
    }

    fn decode(key: &[u8], value: &[u8]) -> Result<Self, DbError> {
        let k = VisibilityKey::decode(key)
            .map_err(|e| DbError::Storage(e.to_string()))?;
        let hidden = value.first().copied().unwrap_or(0) == 1;
        Ok(Self {
            branch_id: k.branch_id,
            tx_id: k.tx_id,
            kind: k.item_kind,
            item_id: k.item_id,
            hidden,
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
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);

        let entry = VisibilityEntry {
            branch_id: Uuid::now_v7(),
            tx_id: Uuid::now_v7(),
            kind: KIND,
            item_id: Uuid::now_v7(),
            hidden: true,
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<VisibilityEntry>(&entry.encode_key()).unwrap().unwrap();
        assert!(found.hidden);
        assert_eq!(found.item_id, entry.item_id);
        assert_eq!(found.kind, KIND);
    }

    #[test]
    fn visible_entry() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);

        let entry = VisibilityEntry {
            branch_id: Uuid::now_v7(),
            tx_id: Uuid::now_v7(),
            kind: KIND,
            item_id: Uuid::now_v7(),
            hidden: false,
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<VisibilityEntry>(&entry.encode_key()).unwrap().unwrap();
        assert!(!found.hidden);
    }
}
