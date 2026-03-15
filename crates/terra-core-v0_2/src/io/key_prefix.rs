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

    /// Encode the upper bound for reverse scans.
    ///
    /// Default: pads `encode()` with `0xFF` up to `Key::SIZE`.
    /// Full prefixes (where `SIZE == Key::SIZE`) return `encode()` unchanged.
    fn encode_upper_bound(&self) -> Vec<u8> {
        let mut bytes = self.encode();
        let pad = Self::Key::SIZE.saturating_sub(bytes.len());
        bytes.extend(std::iter::repeat(0xFFu8).take(pad));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::slug::Slug;
    use crate::io::storage_key::{StorageKey, KeyError};

    #[derive(Debug, Clone)]
    pub struct TestKey48;
    impl StorageKey for TestKey48 {
        const SIZE: usize = 48;
        fn encode(&self) -> Vec<u8> { vec![0; 48] }
        fn decode(_: &[u8]) -> Result<Self, KeyError> { Ok(Self) }
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
        let p1 = TestBranchPrefix { branch: slug.clone() };
        let p2 = TestBranchPrefix { branch: slug };
        assert_eq!(p1.encode(), p2.encode());
    }

    #[test]
    fn upper_bound_pads_to_key_size() {
        let prefix = TestBranchPrefix {
            branch: "main".parse().unwrap(),
        };
        let upper = prefix.encode_upper_bound();
        assert_eq!(upper.len(), 48);
        assert_eq!(&upper[..16], &prefix.encode()[..]);
        assert!(upper[16..].iter().all(|&b| b == 0xFF));
    }

    #[test]
    fn full_prefix_upper_bound_equals_encode() {
        let prefix = TestBranchEntityPrefix {
            branch: "main".parse().unwrap(),
            entity: "test".parse().unwrap(),
        };
        // SIZE(32) < Key::SIZE(48), so there's still padding
        let upper = prefix.encode_upper_bound();
        assert_eq!(upper.len(), 48);
        assert_eq!(&upper[..32], &prefix.encode()[..]);
        assert!(upper[32..].iter().all(|&b| b == 0xFF));
    }
}
