//! Branch record entry.
//!
//! Key: `branch_id(16)` = 16 bytes.
//! Value: JSON with slug, meta, created_from_tx, ancestry.
//! Not versioned — branches are immutable after creation.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::io::{DbItem, DbError};

const CF_BRANCH_MAIN: &str = "branch_main";

/// Maximum branch ancestry depth.
pub const MAX_BRANCH_DEPTH: usize = 8;

/// A branch record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchEntry {
    pub id: Uuid,
    pub slug: String,
    /// Why this branch was created — dynamic metadata.
    pub meta: serde_json::Map<String, serde_json::Value>,
    /// Transaction from which this branch was created. `Uuid::nil()` = genesis.
    pub created_from_tx: Uuid,
    /// Precomputed ancestry: `[(branch_id, branch_point_tx)]`.
    /// First entry is self with `Uuid::max()`, last is main.
    pub ancestry: Vec<(Uuid, Uuid)>,
}

impl DbItem for BranchEntry {
    fn cf() -> &'static str {
        CF_BRANCH_MAIN
    }

    fn encode_key(&self) -> Vec<u8> {
        self.id.as_bytes().to_vec()
    }

    fn encode_value(&self) -> Result<Vec<u8>, DbError> {
        serde_json::to_vec(self).map_err(|e| DbError::Storage(e.to_string()))
    }

    fn decode(key: &[u8], value: &[u8]) -> Result<Self, DbError> {
        let _id = Uuid::from_slice(key)
            .map_err(|e| DbError::Storage(e.to_string()))?;
        serde_json::from_slice(value).map_err(|e| DbError::Storage(e.to_string()))
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
            id,
            slug: "exploration".into(),
            meta: serde_json::Map::new(),
            created_from_tx: Uuid::nil(),
            ancestry: vec![(id, Uuid::max()), (Uuid::nil(), Uuid::max())],
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<BranchEntry>(&entry.encode_key()).unwrap().unwrap();
        assert_eq!(found.id, id);
        assert_eq!(found.slug, "exploration");
        assert_eq!(found.ancestry.len(), 2);
    }
}
