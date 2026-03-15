//! Trait and macro for scan prefix keys.
//!
//! A prefix key is an address for range scans — it encodes only the fixed
//! part (hashes for Slug fields, raw bytes for Uuid/i64/u8). No suffix,
//! no decode. Used with `scan`/`scan_rev`.

/// Trait for prefix keys used in range scans.
///
/// Unlike `StorageKey`, a prefix has no suffix and no decode.
/// `encode()` returns only fixed-size bytes suitable for RocksDB seek.
pub trait KeyPrefix {
    /// Size of the encoded prefix in bytes.
    const SIZE: usize;

    /// Encode the prefix as fixed-size bytes.
    fn encode(&self) -> Vec<u8>;
}

/// Declares a prefix key struct with fixed-size encode only.
///
/// Supported field types: `Uuid` (16 bytes), `Slug` (16 bytes hash),
/// `i64` (8 bytes big-endian), `u8` (1 byte).
///
/// ```ignore
/// prefix_key! {
///     pub struct BranchPrefix {
///         branch: Slug,
///     }
/// }
/// // Encodes as 16 bytes: hash(branch)
/// ```
macro_rules! prefix_key {
    (
        $vis:vis struct $name:ident {
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

    prefix_key! {
        pub struct TestBranchPrefix {
            branch: Slug,
        }
    }

    prefix_key! {
        pub struct TestBranchEntityPrefix {
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
        assert_eq!(bytes.len(), 16); // no suffix
    }

    #[test]
    fn encode_deterministic() {
        let slug: Slug = "main".parse().unwrap();
        let p1 = TestBranchPrefix { branch: slug.clone() };
        let p2 = TestBranchPrefix { branch: slug };
        assert_eq!(p1.encode(), p2.encode());
    }
}
