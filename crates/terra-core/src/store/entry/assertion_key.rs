//! Assertion addressing: branch hash | entity hash | (1 | segment hash)* | 0 | tx.
//! Names follow the usual suffix separator. Path-end 0 sorts before child marker 1;
//! marker 2 is an exclusive subtree successor. Empty legacy segments are hashed too.
use crate::domain::property_path;
use crate::io::storage_key::{KeyError, StorageKey};
use crate::io::{KeyPrefix, Slug};
use crate::store::versioned_key::VersionedKey;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssertionKey {
    pub branch: Slug,
    pub entity: Slug,
    pub prop: Slug,
    pub tx_id: Uuid,
}
fn path_bytes(path: &str, bytes: &mut Vec<u8>) {
    for part in property_path::segments(path) {
        bytes.push(1);
        bytes.extend_from_slice(Slug::hash_text(part).as_bytes());
    }
}
impl StorageKey for AssertionKey {
    // Minimum address size, not an offset: actual path length is variable.
    const SIZE: usize = 49;
    fn encode_fixed(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(self.branch.hash().as_bytes());
        bytes.extend_from_slice(self.entity.hash().as_bytes());
        if self.prop == Slug::max() {
            bytes.push(255);
        } else {
            if self.prop != Slug::min() {
                path_bytes(self.prop.as_str(), &mut bytes);
            }
            bytes.push(0);
        }
        bytes.extend_from_slice(self.tx_id.as_bytes());
        bytes
    }
    fn encode(&self) -> Vec<u8> {
        let mut bytes = self.encode_fixed();
        bytes.push(0);
        for slug in [&self.branch, &self.entity, &self.prop] {
            bytes.push(slug.len() as u8);
            bytes.extend_from_slice(slug.as_str().as_bytes());
        }
        bytes
    }
    fn decode(bytes: &[u8]) -> Result<Self, KeyError> {
        let bad = || KeyError("invalid segmented assertion key".into());
        if bytes.len() < Self::SIZE + 1 {
            return Err(bad());
        }
        let mut pos = 32;
        let mut segments = 0;
        loop {
            match bytes.get(pos) {
                Some(1) => {
                    pos += 17;
                    segments += 1;
                }
                Some(0) => {
                    pos += 1;
                    break;
                }
                _ => return Err(bad()),
            }
        }
        if segments == 0 {
            return Err(bad());
        }
        let tx_id =
            Uuid::from_slice(bytes.get(pos..pos + 16).ok_or_else(bad)?).map_err(|_| bad())?;
        pos += 16;
        if bytes.get(pos) != Some(&0) {
            return Err(bad());
        }
        pos += 1;
        let mut slug = || -> Result<Slug, KeyError> {
            let len = *bytes.get(pos).ok_or_else(bad)? as usize;
            pos += 1;
            let text = std::str::from_utf8(bytes.get(pos..pos + len).ok_or_else(bad)?)
                .map_err(|_| bad())?;
            pos += len;
            text.parse().map_err(|_| bad())
        };
        let key = Self {
            branch: slug()?,
            entity: slug()?,
            prop: slug()?,
            tx_id,
        };
        if pos != bytes.len() || property_path::depth(key.prop.as_str()) != segments {
            return Err(bad());
        }
        Ok(key)
    }
    fn nil() -> Self {
        Self {
            branch: Slug::min(),
            entity: Slug::min(),
            prop: Slug::min(),
            tx_id: Uuid::nil(),
        }
    }
    fn max() -> Self {
        Self {
            branch: Slug::max(),
            entity: Slug::max(),
            prop: Slug::max(),
            tx_id: Uuid::max(),
        }
    }
}
impl VersionedKey for AssertionKey {
    fn branch(&self) -> &Slug {
        &self.branch
    }
    fn tx_id(&self) -> Uuid {
        self.tx_id
    }
    fn set_branch(&mut self, branch: Slug) {
        self.branch = branch;
    }
    fn set_tx_id(&mut self, tx_id: Uuid) {
        self.tx_id = tx_id;
    }
}

/// Explicit variable-length subtree bounds, never padded to StorageKey::SIZE.
pub(crate) struct AssertionRange {
    lower: Vec<u8>,
    upper: Vec<u8>,
}
impl AssertionRange {
    pub fn subtree(branch: &Slug, entity: &Slug, path: Option<&str>) -> Self {
        let mut lower = Vec::new();
        lower.extend_from_slice(branch.hash().as_bytes());
        lower.extend_from_slice(entity.hash().as_bytes());
        if let Some(path) = path {
            path_bytes(path, &mut lower);
        }
        let mut upper = lower.clone();
        upper.push(2);
        Self { lower, upper }
    }
    pub fn after_subtree(branch: &Slug, entity: &Slug, path: &str) -> Self {
        let range = Self::subtree(branch, entity, Some(path));
        Self {
            lower: range.upper.clone(),
            upper: range.upper,
        }
    }
}
impl KeyPrefix for AssertionRange {
    type Key = AssertionKey;
    const SIZE: usize = 0;
    fn encode(&self) -> Vec<u8> {
        self.lower.clone()
    }
    fn encode_lower_bound(&self) -> Vec<u8> {
        self.lower.clone()
    }
    fn encode_upper_bound(&self) -> Vec<u8> {
        self.upper.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(path: &str, tx: u128) -> AssertionKey {
        AssertionKey {
            branch: "main".parse().unwrap(),
            entity: "test".parse().unwrap(),
            prop: path.parse().unwrap(),
            tx_id: Uuid::from_u128(tx),
        }
    }
    #[test]
    fn roundtrip_and_exact_subtree_bounds() {
        let paths = ["a", "a.b", "a.b.c", "ab", "a..b", ".a", "a."];
        for p in paths {
            let k = key(p, 4);
            assert_eq!(AssertionKey::decode(&k.encode()).unwrap(), k);
            let exact = AssertionKey::bound().with_prefix(|b| {
                b.branch = k.branch.clone();
                b.entity = k.entity.clone();
                b.prop = k.prop.clone();
            });
            for other in paths {
                let bytes = key(other, 5).encode();
                assert_eq!(
                    bytes >= exact.encode_lower_bound() && bytes <= exact.encode_upper_bound(),
                    p == other
                );
            }
            let subtree = AssertionRange::subtree(&k.branch, &k.entity, Some(p));
            for other in paths {
                let bytes = key(other, 5).encode();
                assert_eq!(
                    bytes >= subtree.encode_lower_bound() && bytes <= subtree.encode_upper_bound(),
                    crate::domain::property_tree::in_property_subtree(other, p)
                );
            }
        }
        let deep = "a.".repeat(127) + "a";
        assert_eq!(
            AssertionKey::decode(&key(&deep, 7).encode()).unwrap(),
            key(&deep, 7)
        );
    }
    #[test]
    fn malformed_and_legacy_keys_are_rejected() {
        let bytes = key("a.b", 4).encode();
        for n in 0..bytes.len() {
            assert!(AssertionKey::decode(&bytes[..n]).is_err());
        }
        let mut bad = bytes.clone();
        bad[32] = 3;
        assert!(AssertionKey::decode(&bad).is_err());
        let mut bad = bytes.clone();
        bad.push(0);
        assert!(AssertionKey::decode(&bad).is_err());
    }
}
