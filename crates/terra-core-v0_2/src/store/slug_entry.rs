//! Unified slug → UUID index.
//!
//! Single column family shared by all sluggable types.
//! Key: `kind(16) | branch_id(16) | slug_bytes`. Value: UUID (16 bytes).
//!
//! `kind` is a UUID that identifies the namespace (entity, branch, managed type, etc.).
//! Branch-aware lookups walk the ancestry chain.

use uuid::Uuid;

use crate::io::{DbItem, DbError};

const CF_SLUGS: &str = "slugs";

/// A slug index entry ready for storage.
pub struct SlugEntry {
    pub kind: Uuid,
    pub branch_id: Uuid,
    pub slug: String,
    pub id: Uuid,
}

impl DbItem for SlugEntry {
    fn cf() -> &'static str {
        CF_SLUGS
    }

    fn encode_key(&self) -> Vec<u8> {
        let mut key = Vec::with_capacity(32 + self.slug.len());
        key.extend_from_slice(self.kind.as_bytes());
        key.extend_from_slice(self.branch_id.as_bytes());
        key.extend_from_slice(self.slug.as_bytes());
        key
    }

    fn encode_value(&self) -> Result<Vec<u8>, DbError> {
        Ok(self.id.as_bytes().to_vec())
    }

    fn decode(key: &[u8], value: &[u8]) -> Result<Self, DbError> {
        if key.len() < 32 {
            return Err(DbError::Storage("slug key too short".into()));
        }
        let kind = Uuid::from_slice(&key[..16])
            .map_err(|e| DbError::Storage(e.to_string()))?;
        let branch_id = Uuid::from_slice(&key[16..32])
            .map_err(|e| DbError::Storage(e.to_string()))?;
        let slug = String::from_utf8(key[32..].to_vec())
            .map_err(|e| DbError::Storage(e.to_string()))?;
        let id = Uuid::from_slice(value)
            .map_err(|e| DbError::Storage(e.to_string()))?;
        Ok(Self { kind, branch_id, slug, id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::TerraDb;

    fn open_db(dir: &tempfile::TempDir) -> TerraDb {
        TerraDb::builder(dir.path())
            .with::<SlugEntry>()
            .open()
            .unwrap()
    }

    const KIND_ENTITY: Uuid = Uuid::from_u128(0);
    const KIND_BRANCH: Uuid = Uuid::from_u128(1);

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);

        let entry = SlugEntry {
            kind: KIND_ENTITY,
            branch_id: Uuid::now_v7(),
            slug: "my-entity".to_string(),
            id: Uuid::now_v7(),
        };

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<SlugEntry>(&entry.encode_key()).unwrap().unwrap();
        assert_eq!(found.id, entry.id);
        assert_eq!(found.slug, "my-entity");
        assert_eq!(found.kind, KIND_ENTITY);
        assert_eq!(found.branch_id, entry.branch_id);
    }

    #[test]
    fn different_kinds_are_independent() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);

        let branch = Uuid::now_v7();
        let id1 = Uuid::now_v7();
        let id2 = Uuid::now_v7();

        let e1 = SlugEntry { kind: KIND_ENTITY, branch_id: branch, slug: "same".into(), id: id1 };
        let e2 = SlugEntry { kind: KIND_BRANCH, branch_id: branch, slug: "same".into(), id: id2 };

        let mut batch = db.batch();
        batch.put(&e1).unwrap();
        batch.put(&e2).unwrap();
        batch.commit().unwrap();

        assert_eq!(db.get::<SlugEntry>(&e1.encode_key()).unwrap().unwrap().id, id1);
        assert_eq!(db.get::<SlugEntry>(&e2.encode_key()).unwrap().unwrap().id, id2);
    }
}
