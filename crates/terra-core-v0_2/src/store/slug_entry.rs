//! Unified slug → UUID index.
//!
//! Single column family shared by all sluggable types.
//! Key: `branch_id(16) | kind(16) | slug_hash(16)` = 48 bytes (fixed).
//! Value: `uuid(16) | slug_bytes` (binary, no JSON).
//!
//! `slug_hash` is UUID v5 derived from slug string (deterministic).
//! Collision check: on write, if key exists, compare stored slug with new slug.
//!
//! `kind` is a UUID that identifies the namespace (entity, branch, managed type, etc.).
//! Branch-aware lookups walk the ancestry chain.

use uuid::Uuid;

use crate::io::{DbItem, DbError};
use crate::io::storage_key::storage_key;
use crate::io::storage_value::StorageValue;

const CF_SLUGS: &str = "slugs";

/// Namespace UUID for slug hashing (UUID v5).
const SLUG_HASH_NAMESPACE: Uuid = Uuid::from_u128(0xA1B2C3D4_E5F6_7890_ABCD_EF1234567890);

storage_key! {
    pub struct SlugKey(48) {
        branch_id: Uuid,
        kind: Uuid,
        slug_hash: Uuid,
    }
    prefixes {
        prefix_branch(branch_id: Uuid) -> 16,
        prefix_branch_kind(branch_id: Uuid, kind: Uuid) -> 32,
    }
}

/// Compute deterministic hash for a slug string.
pub fn hash_slug(slug: &str) -> Uuid {
    Uuid::new_v5(&SLUG_HASH_NAMESPACE, slug.as_bytes())
}

/// Slug value: uuid + original slug for collision check.
#[derive(Debug, Clone)]
pub struct SlugValue {
    pub id: Uuid,
    pub slug: String,
}

impl StorageValue for SlugValue {
    fn encode(&self) -> Result<Vec<u8>, DbError> {
        let mut val = Vec::with_capacity(16 + self.slug.len());
        val.extend_from_slice(self.id.as_bytes());
        val.extend_from_slice(self.slug.as_bytes());
        Ok(val)
    }

    fn decode(bytes: &[u8]) -> Result<Self, DbError> {
        if bytes.len() < 16 {
            return Err(DbError::Storage("slug value too short".into()));
        }
        let id = Uuid::from_slice(&bytes[..16])
            .map_err(|e| DbError::Storage(e.to_string()))?;
        let slug = String::from_utf8(bytes[16..].to_vec())
            .map_err(|e| DbError::Storage(e.to_string()))?;
        Ok(Self { id, slug })
    }
}

/// Slug entry = key + value.
#[derive(Debug, Clone)]
pub struct SlugEntry {
    pub key: SlugKey,
    pub value: SlugValue,
}

impl SlugEntry {
    /// Create a new slug entry from components.
    pub fn new(branch_id: Uuid, kind: Uuid, slug: &str, id: Uuid) -> Self {
        Self {
            key: SlugKey {
                branch_id,
                kind,
                slug_hash: hash_slug(slug),
            },
            value: SlugValue {
                id,
                slug: slug.to_string(),
            },
        }
    }
}

impl DbItem for SlugEntry {
    type Key = SlugKey;
    type Value = SlugValue;

    fn cf() -> &'static str {
        CF_SLUGS
    }

    fn key(&self) -> &SlugKey {
        &self.key
    }

    fn value(&self) -> &SlugValue {
        &self.value
    }

    fn from_parts(key: SlugKey, value: SlugValue) -> Self {
        Self { key, value }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::storage_key::StorageKey;
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

        let entry = SlugEntry::new(Uuid::now_v7(), KIND_ENTITY, "my-entity", Uuid::now_v7());

        let mut batch = db.batch();
        batch.put(&entry).unwrap();
        batch.commit().unwrap();

        let found = db.get::<SlugEntry>(&entry.key).unwrap().unwrap();
        assert_eq!(found.value.id, entry.value.id);
        assert_eq!(found.value.slug, "my-entity");
    }

    #[test]
    fn different_kinds_are_independent() {
        let dir = tempfile::tempdir().unwrap();
        let db = open_db(&dir);

        let branch = Uuid::now_v7();
        let id1 = Uuid::now_v7();
        let id2 = Uuid::now_v7();

        let e1 = SlugEntry::new(branch, KIND_ENTITY, "same", id1);
        let e2 = SlugEntry::new(branch, KIND_BRANCH, "same", id2);

        let mut batch = db.batch();
        batch.put(&e1).unwrap();
        batch.put(&e2).unwrap();
        batch.commit().unwrap();

        assert_eq!(db.get::<SlugEntry>(&e1.key).unwrap().unwrap().value.id, id1);
        assert_eq!(db.get::<SlugEntry>(&e2.key).unwrap().unwrap().value.id, id2);
    }

    #[test]
    fn hash_is_deterministic() {
        let h1 = hash_slug("test-slug");
        let h2 = hash_slug("test-slug");
        assert_eq!(h1, h2);
    }

    #[test]
    fn different_slugs_different_hashes() {
        let h1 = hash_slug("alpha");
        let h2 = hash_slug("beta");
        assert_ne!(h1, h2);
    }

    #[test]
    fn fixed_key_size() {
        let entry = SlugEntry::new(Uuid::now_v7(), KIND_ENTITY, "any-length-slug-here", Uuid::now_v7());
        assert_eq!(entry.key().encode().len(), 48);
    }
}
