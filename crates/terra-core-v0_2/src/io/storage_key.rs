//! Fixed-size binary storage keys with optional slug suffix.
//!
//! # Example
//!
//! ```ignore
//! storage_key! {
//!     pub struct SchemaTypeKey {
//!         branch_id: Uuid,
//!         type_id: Slug,
//!         tx_id: Uuid,
//!     }
//! }
//! // Fixed part (SIZE = 48): branch_id(16) | hash(type_id)(16) | tx_id(16)
//! // Suffix: len(1) | type_id slug bytes
//! ```
//!
//! Supported field types:
//! - `Uuid` (16 bytes) — raw UUID in fixed part
//! - `i64` (8 bytes) — big-endian in fixed part
//! - `u8` (1 byte) — single byte in fixed part
//! - `Slug` (16 bytes fixed + variable suffix) — UUID v5 hash in fixed part,
//!   original string as length-prefixed suffix

/// Error decoding a storage key from bytes.
#[derive(Debug, thiserror::Error)]
#[error("key decode error: {0}")]
pub struct KeyError(pub String);

/// Separator byte between fixed part and slug suffix.
///
/// Encoded keys: `fixed | 0x00 | suffix`. Upper bounds for reverse seek
/// use `fixed | 0x01` to land past any real key with the same fixed prefix.
pub const SUFFIX_SEPARATOR: u8 = 0x00;

/// Upper-bound sentinel — always greater than `SUFFIX_SEPARATOR | any suffix`.
pub const SUFFIX_SEPARATOR_UPPER: u8 = 0x01;

/// Trait for storage keys with a fixed-size prefix and optional variable suffix.
///
/// `SIZE` is the fixed part size. Encoded keys are always
/// `fixed | SUFFIX_SEPARATOR | slug_suffixes`.
pub trait StorageKey: Sized {
    const SIZE: usize;

    fn encode(&self) -> Vec<u8>;
    fn decode(bytes: &[u8]) -> Result<Self, KeyError>;

    /// Encode only the fixed-size part (hashes for Slug fields, raw bytes for others).
    /// No separator, no slug suffixes.
    fn encode_fixed(&self) -> Vec<u8>;

    /// All-minimum sentinel: every field at its lowest possible value.
    fn nil() -> Self;

    /// All-maximum sentinel: every field at its highest possible value.
    fn max() -> Self;

    /// Full-range `KeyBound` for this key type (`nil..=max`).
    fn bound() -> crate::io::key_prefix::KeyBound<Self> {
        crate::io::key_prefix::KeyBound::new()
    }
}

/// Declares a storage key struct with automatic encode/decode.
///
/// Size of the fixed part is computed from field types at compile time.
/// `Slug` fields store a UUID v5 hash in the fixed part and the original
/// string as a `u8`-length-prefixed suffix after all fixed fields.
macro_rules! storage_key {
    (
        $vis:vis struct $name:ident {
            $( $field:ident : $ty:ident ),+ $(,)?
        }
    ) => {
        #[derive(Debug, Clone, PartialEq, Eq)]
        $vis struct $name {
            $( pub $field: storage_key!(@rust_type $ty) ),+
        }

        impl $crate::io::storage_key::StorageKey for $name {
            const SIZE: usize = 0 $( + storage_key!(@field_size $ty) )+;

            fn encode_fixed(&self) -> Vec<u8> {
                let mut buf = vec![0u8; Self::SIZE];
                let mut _off: usize = 0;
                $( storage_key!(@encode_fixed buf, _off, self.$field, $ty); )+
                buf
            }

            fn encode(&self) -> Vec<u8> {
                let _suffix_size: usize = 0 $( + storage_key!(@suffix_size self.$field, $ty) )+;
                let mut buf = self.encode_fixed();
                buf.reserve(1 + _suffix_size);
                buf.push($crate::io::storage_key::SUFFIX_SEPARATOR);
                $( storage_key!(@encode_suffix buf, self.$field, $ty); )+
                buf
            }

            fn decode(bytes: &[u8]) -> Result<Self, $crate::io::storage_key::KeyError> {
                if bytes.len() < Self::SIZE + 1 {
                    return Err($crate::io::storage_key::KeyError(
                        format!("{} key too short: {} < {}", stringify!($name), bytes.len(), Self::SIZE + 1)
                    ));
                }
                let mut _off: usize = 0;
                let mut _suf: usize = Self::SIZE + 1; // skip separator
                $( let $field = storage_key!(@decode bytes, _off, _suf, $ty)?; )+
                Ok(Self { $( $field ),+ })
            }

            fn nil() -> Self {
                Self { $( $field: storage_key!(@nil_value $ty) ),+ }
            }

            fn max() -> Self {
                Self { $( $field: storage_key!(@max_value $ty) ),+ }
            }
        }
    };

    // --- Type mappings ---
    (@rust_type Uuid) => { uuid::Uuid };
    (@rust_type i64) => { i64 };
    (@rust_type u8) => { u8 };
    (@rust_type Slug) => { $crate::io::slug::Slug };

    // --- Fixed part sizes ---
    (@field_size Uuid) => { 16 };
    (@field_size i64) => { 8 };
    (@field_size u8) => { 1 };
    (@field_size Slug) => { 16 };

    // --- Suffix sizes (runtime) ---
    (@suffix_size $val:expr, Uuid) => { 0usize };
    (@suffix_size $val:expr, i64) => { 0usize };
    (@suffix_size $val:expr, u8) => { 0usize };
    (@suffix_size $val:expr, Slug) => { 1 + $val.len() };

    // --- Encode fixed part ---
    (@encode_fixed $buf:ident, $off:ident, $val:expr, Uuid) => {
        $buf[$off..$off + 16].copy_from_slice($val.as_bytes());
        $off += 16;
    };
    (@encode_fixed $buf:ident, $off:ident, $val:expr, i64) => {
        $buf[$off..$off + 8].copy_from_slice(&$val.to_be_bytes());
        $off += 8;
    };
    (@encode_fixed $buf:ident, $off:ident, $val:expr, u8) => {
        $buf[$off] = $val;
        $off += 1;
    };
    (@encode_fixed $buf:ident, $off:ident, $val:expr, Slug) => {
        let _hash = $val.hash();
        $buf[$off..$off + 16].copy_from_slice(_hash.as_bytes());
        $off += 16;
    };

    // --- Encode suffix (Slug only, others are no-op) ---
    (@encode_suffix $buf:ident, $val:expr, Uuid) => {};
    (@encode_suffix $buf:ident, $val:expr, i64) => {};
    (@encode_suffix $buf:ident, $val:expr, u8) => {};
    (@encode_suffix $buf:ident, $val:expr, Slug) => {
        $buf.push($val.len() as u8);
        $buf.extend_from_slice($val.as_str().as_bytes());
    };

    // --- Decode (two offsets: _off for fixed, _suf for suffix) ---
    (@decode $buf:ident, $off:ident, $suf:ident, Uuid) => {{
        let val = uuid::Uuid::from_slice(&$buf[$off..$off + 16])
            .map_err(|e| $crate::io::storage_key::KeyError(e.to_string()));
        $off += 16;
        val
    }};
    (@decode $buf:ident, $off:ident, $suf:ident, i64) => {{
        let val: Result<i64, $crate::io::storage_key::KeyError> = $buf[$off..$off + 8]
            .try_into()
            .map(i64::from_be_bytes)
            .map_err(|_| $crate::io::storage_key::KeyError("bad i64".into()));
        $off += 8;
        val
    }};
    (@decode $buf:ident, $off:ident, $suf:ident, u8) => {{
        let val: Result<u8, $crate::io::storage_key::KeyError> = Ok($buf[$off]);
        $off += 1;
        val
    }};
    (@decode $buf:ident, $off:ident, $suf:ident, Slug) => {{
        $off += 16; // skip hash in fixed part
        let _len = $buf[$suf] as usize;
        $suf += 1;
        let _slug_str = std::str::from_utf8(&$buf[$suf..$suf + _len])
            .map_err(|e| $crate::io::storage_key::KeyError(e.to_string()))?;
        $suf += _len;
        let val: Result<$crate::io::slug::Slug, $crate::io::storage_key::KeyError> = _slug_str.parse()
            .map_err(|e: $crate::io::slug::SlugError| $crate::io::storage_key::KeyError(e.to_string()));
        val
    }};

    // --- Nil (minimum) values ---
    (@nil_value Uuid) => { uuid::Uuid::nil() };
    (@nil_value Slug) => { $crate::io::slug::Slug::min() };
    (@nil_value u8)   => { 0u8 };
    (@nil_value i64)  => { i64::MIN };

    // --- Max values ---
    (@max_value Uuid) => { uuid::Uuid::max() };
    (@max_value Slug) => { $crate::io::slug::Slug::max() };
    (@max_value u8)   => { 0xFFu8 };
    (@max_value i64)  => { i64::MAX };
}

pub(crate) use storage_key;

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    use crate::io::slug::Slug;

    storage_key! {
        pub(crate) struct TestKey {
            branch_id: Uuid,
            entity_id: Uuid,
            timestamp_us: i64,
        }
    }

    #[test]
    fn auto_size() {
        assert_eq!(TestKey::SIZE, 40);
    }

    #[test]
    fn roundtrip() {
        let key = TestKey {
            branch_id: Uuid::nil(),
            entity_id: Uuid::from_u128(42),
            timestamp_us: 1_700_000_000_000_000,
        };
        let encoded = key.encode();
        assert_eq!(encoded.len(), 41); // 40 fixed + 1 separator

        let decoded = TestKey::decode(&encoded).unwrap();
        assert_eq!(decoded, key);
    }

    storage_key! {
        pub(crate) struct TestKeyWithU8 {
            branch_id: Uuid,
            kind: u8,
            item_id: Uuid,
        }
    }

    #[test]
    fn u8_auto_size() {
        assert_eq!(TestKeyWithU8::SIZE, 33);
    }

    #[test]
    fn u8_roundtrip() {
        let key = TestKeyWithU8 {
            branch_id: Uuid::nil(),
            kind: 42,
            item_id: Uuid::from_u128(7),
        };
        let encoded = key.encode();
        assert_eq!(encoded.len(), 34); // 33 fixed + 1 separator
        assert_eq!(encoded[16], 42);

        let decoded = TestKeyWithU8::decode(&encoded).unwrap();
        assert_eq!(decoded, key);
    }

    #[test]
    fn decode_too_short() {
        assert!(TestKey::decode(&[0u8; 10]).is_err());
    }

    #[test]
    fn keys_sort_correctly() {
        let k1 = TestKey {
            branch_id: Uuid::nil(),
            entity_id: Uuid::from_u128(1),
            timestamp_us: 100,
        };
        let k2 = TestKey {
            branch_id: Uuid::nil(),
            entity_id: Uuid::from_u128(1),
            timestamp_us: 200,
        };
        assert!(k1.encode() < k2.encode());
    }

    // --- Slug field tests ---

    storage_key! {
        pub(crate) struct SlugKey {
            branch_id: Uuid,
            name: Slug,
        }
    }

    #[test]
    fn slug_fixed_size() {
        // Fixed part: Uuid(16) + hash(16) = 32
        assert_eq!(SlugKey::SIZE, 32);
    }

    #[test]
    fn slug_roundtrip() {
        let slug: Slug = "my-entity".parse().unwrap();
        let key = SlugKey {
            branch_id: Uuid::from_u128(1),
            name: slug.clone(),
        };
        let encoded = key.encode();
        // Fixed(32) + separator(1) + len(1) + "my-entity"(9) = 43
        assert_eq!(encoded.len(), 43);

        let decoded = SlugKey::decode(&encoded).unwrap();
        assert_eq!(decoded.branch_id, key.branch_id);
        assert_eq!(decoded.name, slug);
    }

    storage_key! {
        pub(crate) struct MultiSlugKey {
            branch_id: Uuid,
            type_name: Slug,
            prop_name: Slug,
        }
    }

    #[test]
    fn multi_slug_roundtrip() {
        let t: Slug = "person".parse().unwrap();
        let p: Slug = "age".parse().unwrap();
        let key = MultiSlugKey {
            branch_id: Uuid::from_u128(1),
            type_name: t.clone(),
            prop_name: p.clone(),
        };
        let encoded = key.encode();
        // Fixed: 16 + 16 + 16 = 48. Separator: 1. Suffix: (1+6) + (1+3) = 11. Total = 60.
        assert_eq!(encoded.len(), 60);

        let decoded = MultiSlugKey::decode(&encoded).unwrap();
        assert_eq!(decoded.type_name, t);
        assert_eq!(decoded.prop_name, p);
    }

    #[test]
    fn slug_keys_sort_by_hash() {
        let s1: Slug = "alpha".parse().unwrap();
        let s2: Slug = "beta".parse().unwrap();
        let k1 = SlugKey { branch_id: Uuid::nil(), name: s1 };
        let k2 = SlugKey { branch_id: Uuid::nil(), name: s2 };
        // Sort order is by hash, not by slug string
        let e1 = k1.encode();
        let e2 = k2.encode();
        assert_ne!(e1, e2);
    }

    #[test]
    fn slug_hash_in_fixed_part() {
        let slug: Slug = "test".parse().unwrap();
        let key = SlugKey {
            branch_id: Uuid::nil(),
            name: slug.clone(),
        };
        let encoded = key.encode();
        // Hash should be at offset 16..32
        let hash_bytes = &encoded[16..32];
        assert_eq!(hash_bytes, slug.hash().as_bytes());
    }

    #[test]
    fn encode_fixed_excludes_suffix() {
        let slug: Slug = "my-entity".parse().unwrap();
        let key = SlugKey {
            branch_id: Uuid::from_u128(1),
            name: slug,
        };
        let fixed = key.encode_fixed();
        assert_eq!(fixed.len(), SlugKey::SIZE);
        // Full encode has suffix, fixed does not
        assert!(key.encode().len() > fixed.len());
    }

    #[test]
    fn encode_fixed_matches_encode_prefix() {
        let key = TestKey {
            branch_id: Uuid::from_u128(1),
            entity_id: Uuid::from_u128(2),
            timestamp_us: 42,
        };
        let fixed = key.encode_fixed();
        let full = key.encode();
        // No slug fields → full has fixed + separator byte
        assert_eq!(full.len(), fixed.len() + 1);
        assert_eq!(&full[..fixed.len()], &fixed[..]);
        assert_eq!(full[fixed.len()], 0x00); // SUFFIX_SEPARATOR
    }

    #[test]
    fn nil_all_zeros() {
        let key = TestKey::nil();
        assert_eq!(key.branch_id, Uuid::nil());
        assert_eq!(key.entity_id, Uuid::nil());
        assert_eq!(key.timestamp_us, i64::MIN);
    }

    #[test]
    fn max_all_max() {
        let key = TestKey::max();
        assert_eq!(key.branch_id, Uuid::max());
        assert_eq!(key.entity_id, Uuid::max());
        assert_eq!(key.timestamp_us, i64::MAX);
    }

    #[test]
    fn nil_sorts_before_max() {
        let nil = TestKey::nil().encode();
        let max = TestKey::max().encode();
        assert!(nil < max);
    }

    #[test]
    fn nil_slug_key() {
        let key = SlugKey::nil();
        assert_eq!(key.branch_id, Uuid::nil());
        assert_eq!(key.name.hash().as_bytes(), &[0x00; 16]);
    }

    #[test]
    fn max_slug_key() {
        let key = SlugKey::max();
        assert_eq!(key.branch_id, Uuid::max());
        assert_eq!(key.name.hash().as_bytes(), &[0xFF; 16]);
    }
}
