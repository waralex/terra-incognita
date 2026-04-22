//! Trait and macro for scan prefix keys.
//!
//! A prefix key is an address for range scans — it encodes only the fixed
//! part (hashes for Slug fields, raw bytes for Uuid/i64/u8). No suffix,
//! no decode. Used with `scan`/`scan_rev`.

use crate::io::storage_key::StorageKey;

/// Trait for prefix keys used in range scans.
///
/// Unlike `StorageKey`, a prefix has no suffix and no decode.
/// `encode()` returns only fixed-size bytes suitable for RocksDB seek.
pub trait KeyPrefix {
    /// The key type this prefix is valid for.
    type Key: StorageKey;

    /// Size of the encoded prefix in bytes.
    const SIZE: usize;

    /// Encode the prefix as fixed-size bytes.
    fn encode(&self) -> Vec<u8>;

    /// Encode the lower bound of the scan range.
    ///
    /// Default: pads `encode()` with `0x00` up to `Key::SIZE`, then appends separator.
    fn encode_lower_bound(&self) -> Vec<u8> {
        let mut bytes = self.encode();
        let pad = Self::Key::SIZE.saturating_sub(bytes.len());
        bytes.extend(std::iter::repeat(0x00u8).take(pad));
        bytes.push(crate::io::storage_key::SUFFIX_SEPARATOR);
        bytes
    }

    /// Encode the upper bound of the scan range.
    ///
    /// Default: pads `encode()` with `0xFF` up to `Key::SIZE`, then appends upper sentinel.
    fn encode_upper_bound(&self) -> Vec<u8> {
        let mut bytes = self.encode();
        let pad = Self::Key::SIZE.saturating_sub(bytes.len());
        bytes.extend(std::iter::repeat(0xFFu8).take(pad));
        bytes.push(crate::io::storage_key::SUFFIX_SEPARATOR_UPPER);
        bytes
    }
}

/// Declares a prefix key struct with fixed-size encode only.
///
/// Supported field types: `Uuid` (16 bytes), `Slug` (16 bytes hash),
/// `i64` (8 bytes big-endian), `u8` (1 byte).
///
/// ```ignore
/// prefix_key! {
///     pub struct BranchPrefix for EntityKey {
///         branch: Slug,
///     }
/// }
/// // Encodes as 16 bytes: hash(branch)
/// ```
macro_rules! prefix_key {
    (
        $vis:vis struct $name:ident for $key:ty {
            $( $field:ident : $ty:ident ),+ $(,)?
        }
    ) => {
        #[derive(Debug, Clone, PartialEq, Eq)]
        $vis struct $name {
            $( pub $field: prefix_key!(@rust_type $ty) ),+
        }

        impl $name {
            pub fn new($( $field: prefix_key!(@rust_type $ty) ),+) -> Self {
                Self { $( $field ),+ }
            }
        }

        impl $crate::io::key_prefix::KeyPrefix for $name {
            type Key = $key;
            const SIZE: usize = 0 $( + prefix_key!(@field_size $ty) )+;

            fn encode(&self) -> Vec<u8> {
                let mut buf = vec![0u8; Self::SIZE];
                let mut _off: usize = 0;
                $( prefix_key!(@encode buf, _off, self.$field, $ty); )+
                buf
            }
        }
    };

    (@rust_type Uuid) => { uuid::Uuid };
    (@rust_type i64) => { i64 };
    (@rust_type u8) => { u8 };
    (@rust_type Slug) => { $crate::io::slug::Slug };

    (@field_size Uuid) => { 16 };
    (@field_size i64) => { 8 };
    (@field_size u8) => { 1 };
    (@field_size Slug) => { 16 };

    (@encode $buf:ident, $off:ident, $val:expr, Uuid) => {
        $buf[$off..$off + 16].copy_from_slice($val.as_bytes());
        $off += 16;
    };
    (@encode $buf:ident, $off:ident, $val:expr, i64) => {
        $buf[$off..$off + 8].copy_from_slice(&$val.to_be_bytes());
        $off += 8;
    };
    (@encode $buf:ident, $off:ident, $val:expr, u8) => {
        $buf[$off] = $val;
        $off += 1;
    };
    (@encode $buf:ident, $off:ident, $val:expr, Slug) => {
        let _hash = $val.hash();
        $buf[$off..$off + 16].copy_from_slice(_hash.as_bytes());
        $off += 16;
    };
}

pub(crate) use prefix_key;

/// Generic scan bounds: a (lower, upper) key pair that implements `KeyPrefix`.
///
/// Starts as full range (`K::nil()..=K::max()`). Narrow with `with_prefix`,
/// `with_lower`, `with_upper`.
#[derive(Debug, Clone)]
pub struct KeyBound<K: StorageKey> {
    pub lower: K,
    pub upper: K,
}

impl<K: StorageKey> KeyBound<K> {
    /// Full range: all keys from nil to max.
    pub fn new() -> Self {
        Self {
            lower: K::nil(),
            upper: K::max(),
        }
    }

    /// Apply `f` to both lower and upper (set a shared prefix field).
    pub fn with_prefix(mut self, mut f: impl FnMut(&mut K)) -> Self {
        f(&mut self.lower);
        f(&mut self.upper);
        self
    }

    /// Apply `f` to the lower bound only.
    pub fn with_lower(mut self, mut f: impl FnMut(&mut K)) -> Self {
        f(&mut self.lower);
        self
    }

    /// Apply `f` to the upper bound only.
    pub fn with_upper(mut self, mut f: impl FnMut(&mut K)) -> Self {
        f(&mut self.upper);
        self
    }
}

impl<K: StorageKey> KeyPrefix for KeyBound<K> {
    type Key = K;
    const SIZE: usize = K::SIZE;

    fn encode(&self) -> Vec<u8> {
        self.lower.encode_fixed()
    }

    fn encode_lower_bound(&self) -> Vec<u8> {
        let mut bytes = self.lower.encode_fixed();
        bytes.push(crate::io::storage_key::SUFFIX_SEPARATOR);
        bytes
    }

    fn encode_upper_bound(&self) -> Vec<u8> {
        let mut bytes = self.upper.encode_fixed();
        bytes.push(crate::io::storage_key::SUFFIX_SEPARATOR_UPPER);
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::slug::Slug;
    use crate::io::storage_key::{KeyError, StorageKey};

    #[derive(Debug, Clone)]
    pub struct TestKey48;
    impl StorageKey for TestKey48 {
        const SIZE: usize = 48;
        fn encode(&self) -> Vec<u8> {
            vec![0; 48]
        }
        fn encode_fixed(&self) -> Vec<u8> {
            vec![0; 48]
        }
        fn decode(_: &[u8]) -> Result<Self, KeyError> {
            Ok(Self)
        }
        fn nil() -> Self {
            Self
        }
        fn max() -> Self {
            Self
        }
    }

    prefix_key! {
        pub struct TestBranchPrefix for TestKey48 {
            branch: Slug,
        }
    }

    prefix_key! {
        pub struct TestBranchEntityPrefix for TestKey48 {
            branch: Slug,
            entity: Slug,
        }
    }

    #[test]
    fn size() {
        assert_eq!(TestBranchPrefix::SIZE, 16);
        assert_eq!(TestBranchEntityPrefix::SIZE, 32);
    }

    #[test]
    fn encode_slug_is_hash_only() {
        let prefix = TestBranchPrefix {
            branch: "main".parse().unwrap(),
        };
        let bytes = prefix.encode();
        assert_eq!(bytes.len(), 16);
    }

    #[test]
    fn encode_deterministic() {
        let slug: Slug = "main".parse().unwrap();
        let p1 = TestBranchPrefix {
            branch: slug.clone(),
        };
        let p2 = TestBranchPrefix { branch: slug };
        assert_eq!(p1.encode(), p2.encode());
    }

    #[test]
    fn lower_bound_pads_with_zeros() {
        let prefix = TestBranchPrefix {
            branch: "main".parse().unwrap(),
        };
        let lower = prefix.encode_lower_bound();
        assert_eq!(lower.len(), 49); // 48 + separator
        assert_eq!(&lower[..16], &prefix.encode()[..]);
        assert!(lower[16..].iter().all(|&b| b == 0x00));
    }

    #[test]
    fn upper_bound_pads_to_key_size() {
        let prefix = TestBranchPrefix {
            branch: "main".parse().unwrap(),
        };
        let upper = prefix.encode_upper_bound();
        assert_eq!(upper.len(), 49); // 48 + separator_upper
        assert_eq!(&upper[..16], &prefix.encode()[..]);
        assert!(upper[16..48].iter().all(|&b| b == 0xFF));
        assert_eq!(upper[48], 0x01); // SUFFIX_SEPARATOR_UPPER
    }

    #[test]
    fn full_prefix_upper_bound_equals_encode() {
        let prefix = TestBranchEntityPrefix {
            branch: "main".parse().unwrap(),
            entity: "test".parse().unwrap(),
        };
        // SIZE(32) < Key::SIZE(48), so there's still padding
        let upper = prefix.encode_upper_bound();
        assert_eq!(upper.len(), 49); // 48 + separator_upper
        assert_eq!(&upper[..32], &prefix.encode()[..]);
        assert!(upper[32..48].iter().all(|&b| b == 0xFF));
        assert_eq!(upper[48], 0x01); // SUFFIX_SEPARATOR_UPPER
    }

    // --- KeyBound tests ---

    use crate::io::storage_key::storage_key;

    storage_key! {
        pub struct BoundTestKey {
            branch_id: Uuid,
            entity_id: Uuid,
            tx_id: Uuid,
        }
    }

    #[test]
    fn key_bound_full_range() {
        let bound = KeyBound::<BoundTestKey>::new();
        let lower = bound.encode_lower_bound();
        let upper = bound.encode_upper_bound();
        assert_eq!(lower.len(), BoundTestKey::SIZE + 1);
        assert_eq!(upper.len(), BoundTestKey::SIZE + 1);
        assert!(lower.iter().all(|&b| b == 0x00));
        assert!(upper[..BoundTestKey::SIZE].iter().all(|&b| b == 0xFF));
        assert_eq!(upper[BoundTestKey::SIZE], 0x01); // SUFFIX_SEPARATOR_UPPER
    }

    #[test]
    fn key_bound_with_prefix() {
        let branch = uuid::Uuid::from_u128(42);
        let bound = KeyBound::<BoundTestKey>::new().with_prefix(|k| k.branch_id = branch);
        let lower = bound.encode_lower_bound();
        let upper = bound.encode_upper_bound();
        // Both share the same branch prefix
        assert_eq!(&lower[..16], branch.as_bytes());
        assert_eq!(&upper[..16], branch.as_bytes());
        // Lower has zeros for remaining fields + separator
        assert!(lower[16..].iter().all(|&b| b == 0x00));
        // Upper has 0xFF for remaining fixed fields, then SUFFIX_SEPARATOR_UPPER
        assert!(upper[16..BoundTestKey::SIZE].iter().all(|&b| b == 0xFF));
        assert_eq!(upper[BoundTestKey::SIZE], 0x01);
    }

    #[test]
    fn key_bound_with_lower_upper() {
        let bound = KeyBound::<BoundTestKey>::new()
            .with_lower(|k| k.entity_id = uuid::Uuid::from_u128(10))
            .with_upper(|k| k.entity_id = uuid::Uuid::from_u128(20));
        let lower = bound.encode_lower_bound();
        let upper = bound.encode_upper_bound();
        assert_eq!(&lower[16..32], uuid::Uuid::from_u128(10).as_bytes());
        assert_eq!(&upper[16..32], uuid::Uuid::from_u128(20).as_bytes());
    }

    #[test]
    fn key_bound_size_matches_key() {
        assert_eq!(
            <KeyBound<BoundTestKey> as KeyPrefix>::SIZE,
            BoundTestKey::SIZE
        );
    }
}
