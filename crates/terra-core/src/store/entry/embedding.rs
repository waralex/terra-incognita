//! Embedding entry — vector embedding for an entity at a given transaction.
//!
//! Key: `branch(16) | entity(16) | tx_id(16)` = 48 bytes + slug suffixes.
//! Value: change_id (provenance link) + embedding vector.
//!
//! One entry per entity per transaction. The change_id links back to
//! EntityChangeEntry for full provenance.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::io::storage_value::StorageValue;
use crate::io::{DbError, DbItem};
use crate::store::versioned_key::versioned_key;

const CF_EMBEDDINGS: &str = "embeddings";

versioned_key! {
    pub struct EmbeddingKey {
        entity: Slug,
    }
}

/// Embedding value — provenance link + vector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingValue {
    pub change_id: Uuid,
    pub embedding: Vec<f32>,
}

impl EmbeddingValue {
    pub fn is_active(&self) -> bool {
        !self.embedding.is_empty()
    }
}

impl StorageValue for EmbeddingValue {
    fn encode(&self) -> Result<Vec<u8>, DbError> {
        serde_json::to_vec(self).map_err(|e| DbError::Storage(e.to_string()))
    }

    fn decode(bytes: &[u8]) -> Result<Self, DbError> {
        serde_json::from_slice(bytes).map_err(|e| DbError::Storage(e.to_string()))
    }
}

/// Embedding entry = key + value.
#[derive(Debug, Clone)]
pub struct EmbeddingEntry {
    pub key: EmbeddingKey,
    pub value: EmbeddingValue,
}

impl DbItem for EmbeddingEntry {
    type Key = EmbeddingKey;
    type Value = EmbeddingValue;

    fn cf() -> &'static str {
        CF_EMBEDDINGS
    }

    fn key(&self) -> &EmbeddingKey {
        &self.key
    }

    fn value(&self) -> &EmbeddingValue {
        &self.value
    }

    fn from_parts(key: EmbeddingKey, value: EmbeddingValue) -> Self {
        Self { key, value }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::slug::Slug;
    use crate::io::TerraDb;

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = TerraDb::builder(dir.path())
            .with::<EmbeddingEntry>()
            .open()
            .unwrap();

        let entry = EmbeddingEntry {
            key: EmbeddingKey {
                branch: "main".parse::<Slug>().unwrap(),
                entity: "alice".parse::<Slug>().unwrap(),
                tx_id: Uuid::now_v7(),
            },
            value: EmbeddingValue {
                change_id: Uuid::now_v7(),
                embedding: vec![0.1, 0.2, 0.3, 0.4],
            },
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<EmbeddingEntry>(&entry.key).unwrap().unwrap();
        assert_eq!(found.key.entity, entry.key.entity);
        assert_eq!(found.value.change_id, entry.value.change_id);
        assert_eq!(found.value.embedding, vec![0.1, 0.2, 0.3, 0.4]);
    }
}
