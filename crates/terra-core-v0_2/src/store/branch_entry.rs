//! Branch record entry.
//!
//! Key: `branch_id(16)` = 16 bytes.
//! Value: JSON with slug, meta, created_from_tx.
//! Not versioned — branches are immutable after creation.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::io::{DbItem, DbError};

const CF_BRANCH_MAIN: &str = "branch_main";

/// Branch key.
#[derive(Debug, Clone)]
pub struct BranchKey {
    pub branch_id: Uuid,
}

/// Branch value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchValue {
    pub slug: String,
    pub meta: serde_json::Map<String, serde_json::Value>,
    /// Branch this was forked from. `Uuid::nil()` = forked from main.
    pub parent_branch_id: Uuid,
    /// Transaction on the parent branch at which this branch was created.
    pub created_from_tx: Uuid,
}

/// Branch entry = key + value.
#[derive(Debug, Clone)]
pub struct BranchEntry {
    pub key: BranchKey,
    pub value: BranchValue,
}

impl DbItem for BranchEntry {
    fn cf() -> &'static str {
        CF_BRANCH_MAIN
    }

    fn encode_key(&self) -> Vec<u8> {
        self.key.branch_id.as_bytes().to_vec()
    }

    fn encode_value(&self) -> Result<Vec<u8>, DbError> {
        serde_json::to_vec(&self.value).map_err(|e| DbError::Storage(e.to_string()))
    }

    fn decode(key: &[u8], value: &[u8]) -> Result<Self, DbError> {
        let branch_id = Uuid::from_slice(key)
            .map_err(|e| DbError::Storage(e.to_string()))?;
        let val: BranchValue =
            serde_json::from_slice(value).map_err(|e| DbError::Storage(e.to_string()))?;
        Ok(Self {
            key: BranchKey { branch_id },
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
            .with::<BranchEntry>()
            .open()
            .unwrap();

        let id = Uuid::now_v7();
        let entry = BranchEntry {
            key: BranchKey { branch_id: id },
            value: BranchValue {
                slug: "exploration".into(),
                meta: serde_json::Map::new(),
                parent_branch_id: Uuid::nil(),
                created_from_tx: Uuid::nil(),
            },
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<BranchEntry>(&entry.encode_key()).unwrap().unwrap();
        assert_eq!(found.key.branch_id, id);
        assert_eq!(found.value.slug, "exploration");
    }
}
