//! Touched entry — entities the agent considered relevant in a transaction.
//!
//! Key: `branch(16) | tx_id(16) | entity(16)` = 48 bytes fixed + slug suffixes.
//! Value: reasoning (why this entity was relevant).
//!
//! Used to reconstruct agent context: scan reverse from current tx,
//! collect unique entities until limit is reached.

use serde::{Deserialize, Serialize};

use crate::io::{DbItem, DbError};
use crate::io::storage_key::storage_key;
use crate::io::storage_value::StorageValue;

const CF_TOUCHED: &str = "touched";

storage_key! {
    pub struct TouchedKey {
        branch: Slug,
        tx_id: Uuid,
        entity: Slug,
    }
}

/// Touched value — reasoning for why this entity is in context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TouchedValue {
    pub reasoning: String,
}

impl StorageValue for TouchedValue {
    fn encode(&self) -> Result<Vec<u8>, DbError> {
        serde_json::to_vec(self).map_err(|e| DbError::Storage(e.to_string()))
    }

    fn decode(bytes: &[u8]) -> Result<Self, DbError> {
        serde_json::from_slice(bytes).map_err(|e| DbError::Storage(e.to_string()))
    }
}

/// Touched entry = key + value.
#[derive(Debug, Clone)]
pub struct TouchedEntry {
    pub key: TouchedKey,
    pub value: TouchedValue,
}

impl DbItem for TouchedEntry {
    type Key = TouchedKey;
    type Value = TouchedValue;

    fn cf() -> &'static str {
        CF_TOUCHED
    }

    fn key(&self) -> &TouchedKey {
        &self.key
    }

    fn value(&self) -> &TouchedValue {
        &self.value
    }

    fn from_parts(key: TouchedKey, value: TouchedValue) -> Self {
        Self { key, value }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    use crate::io::slug::Slug;
    use crate::io::TerraDb;

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = TerraDb::builder(dir.path())
            .with::<TouchedEntry>()
            .open()
            .unwrap();

        let entry = TouchedEntry {
            key: TouchedKey {
                branch: "main".parse::<Slug>().unwrap(),
                tx_id: Uuid::now_v7(),
                entity: "alice".parse::<Slug>().unwrap(),
            },
            value: TouchedValue {
                reasoning: "relevant to current investigation".into(),
            },
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<TouchedEntry>(&entry.key).unwrap().unwrap();
        assert_eq!(found.key.entity.as_str(), "alice");
        assert_eq!(found.value.reasoning, "relevant to current investigation");
    }

    #[test]
    fn multiple_entities_per_tx() {
        let dir = tempfile::tempdir().unwrap();
        let db = TerraDb::builder(dir.path())
            .with::<TouchedEntry>()
            .open()
            .unwrap();

        let branch: Slug = "main".parse().unwrap();
        let tx_id = Uuid::now_v7();

        let mut batch = db.batch();
        for (slug, reason) in [("alice", "subject"), ("bob", "witness"), ("server", "infrastructure")] {
            batch.put(&TouchedEntry {
                key: TouchedKey {
                    branch: branch.clone(),
                    tx_id,
                    entity: slug.parse().unwrap(),
                },
                value: TouchedValue { reasoning: reason.into() },
            }).unwrap();
        }
        batch.commit().unwrap();

        // Reverse scan by branch prefix returns all three
        use crate::io::storage_key::StorageKey;
        let bound = TouchedKey::bound()
            .with_prefix(|k| k.branch = branch.clone());
        let mut iter = db.scan_rev::<TouchedEntry>(&bound).unwrap();
        let mut count = 0;
        while let Some(Ok(_)) = iter.next() {
            count += 1;
        }
        assert_eq!(count, 3);
    }
}
