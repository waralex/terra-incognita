//! Assertion log entry — provenance record for an assertion.
//!
//! Key: `entry_id(16)` = 16 bytes.
//! Value: JSON with entity_id, tx_id, properties, reasoning.
//! Append-only, global (not branch-scoped).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::io::{DbItem, DbError};

const CF_ASSERTION_LOG: &str = "assertion_log";

/// Provenance record for an assertion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssertionLogEntry {
    pub id: Uuid,
    pub entity_id: Uuid,
    pub tx_id: Uuid,
    pub properties: serde_json::Value,
    pub reasoning: serde_json::Value,
}

impl DbItem for AssertionLogEntry {
    fn cf() -> &'static str {
        CF_ASSERTION_LOG
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
            .with::<AssertionLogEntry>()
            .open()
            .unwrap();

        let entry = AssertionLogEntry {
            id: Uuid::now_v7(),
            entity_id: Uuid::now_v7(),
            tx_id: Uuid::now_v7(),
            properties: serde_json::json!({"population": 56000000}),
            reasoning: serde_json::json!("census data"),
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<AssertionLogEntry>(&entry.encode_key()).unwrap().unwrap();
        assert_eq!(found.id, entry.id);
        assert_eq!(found.reasoning, serde_json::json!("census data"));
    }
}
