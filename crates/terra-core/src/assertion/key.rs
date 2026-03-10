use super::log::LogError;

/// Trait for fixed-size storage keys with encode/decode.
pub(crate) trait StorageKey: Sized {
    const SIZE: usize;

    fn encode(&self) -> Vec<u8>;
    fn decode(bytes: &[u8]) -> Result<Self, LogError>;
}

/// Declares a fixed-size storage key struct with automatic encode/decode
/// and named prefix methods.
///
/// Supported field types: `Uuid` (16 bytes), `i64` (8 bytes big-endian), `u8` (1 byte).
///
/// ```ignore
/// storage_key! {
///     pub(crate) struct LogKey(56) {
///         branch_id: Uuid,
///         timestamp_us: i64,
///         entry_id: Uuid,
///         entity_id: Uuid,
///     }
///     prefixes {
///         prefix_branch(branch_id: Uuid) -> 16,
///         prefix_branch_ts(branch_id: Uuid, timestamp_us: i64) -> 24,
///     }
/// }
/// ```
macro_rules! storage_key {
    (
        $vis:vis struct $name:ident($size:literal) {
            $( $field:ident : $ty:ident ),+ $(,)?
        }
        $(prefixes {
            $( $prefix_name:ident( $( $pf:ident : $pt:ident ),+ ) -> $prefix_size:literal ),+ $(,)?
        })?
    ) => {
        #[derive(Debug, Clone, PartialEq, Eq)]
        $vis struct $name {
            $( pub $field: storage_key!(@rust_type $ty) ),+
        }

        // Compile-time: sum of field sizes must equal declared size
        const _: () = assert!(
            0 $( + storage_key!(@field_size $ty) )+ == $size,
            concat!("storage key size mismatch for ", stringify!($name))
        );

        impl $crate::assertion::key::StorageKey for $name {
            const SIZE: usize = $size;

            fn encode(&self) -> Vec<u8> {
                let mut buf = vec![0u8; $size];
                let mut _off: usize = 0;
                $( storage_key!(@encode buf, _off, self.$field, $ty); )+
                buf
            }

            fn decode(bytes: &[u8]) -> Result<Self, $crate::assertion::log::LogError> {
                if bytes.len() < $size {
                    return Err($crate::assertion::log::LogError::Storage(
                        format!("{} key too short: {} < {}", stringify!($name), bytes.len(), $size)
                    ));
                }
                let mut _off: usize = 0;
                $( let $field = storage_key!(@decode bytes, _off, $ty)?; )+
                Ok(Self { $( $field ),+ })
            }
        }

        $(
            // Compile-time prefix size checks
            $( const _: () = assert!(
                0 $( + storage_key!(@field_size $pt) )+ == $prefix_size,
                concat!("prefix size mismatch for ", stringify!($prefix_name))
            ); )+

            impl $name {
                $(
                    $vis fn $prefix_name( $( $pf: &storage_key!(@rust_type $pt) ),+ ) -> Vec<u8> {
                        let mut buf = vec![0u8; $prefix_size];
                        let mut _off: usize = 0;
                        $( storage_key!(@encode_ref buf, _off, $pf, $pt); )+
                        buf
                    }
                )+
            }
        )?
    };

    // Map macro type names to Rust types
    (@rust_type Uuid) => { uuid::Uuid };
    (@rust_type i64) => { i64 };
    (@rust_type u8) => { u8 };

    // Field sizes
    (@field_size Uuid) => { 16 };
    (@field_size i64) => { 8 };
    (@field_size u8) => { 1 };

    // Encode owned field
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

    // Encode reference (for prefix functions)
    (@encode_ref $buf:ident, $off:ident, $val:expr, Uuid) => {
        $buf[$off..$off + 16].copy_from_slice($val.as_bytes());
        $off += 16;
    };
    (@encode_ref $buf:ident, $off:ident, $val:expr, i64) => {
        $buf[$off..$off + 8].copy_from_slice(&$val.to_be_bytes());
        $off += 8;
    };
    (@encode_ref $buf:ident, $off:ident, $val:expr, u8) => {
        $buf[$off] = *$val;
        $off += 1;
    };

    // Decode field
    (@decode $buf:ident, $off:ident, Uuid) => {{
        let val = uuid::Uuid::from_slice(&$buf[$off..$off + 16])
            .map_err(|e| $crate::assertion::log::LogError::Storage(e.to_string()));
        $off += 16;
        val
    }};
    (@decode $buf:ident, $off:ident, i64) => {{
        let val: Result<i64, $crate::assertion::log::LogError> = $buf[$off..$off + 8]
            .try_into()
            .map(i64::from_be_bytes)
            .map_err(|_| $crate::assertion::log::LogError::Storage("bad i64".into()));
        $off += 8;
        val
    }};
    (@decode $buf:ident, $off:ident, u8) => {{
        let val: Result<u8, $crate::assertion::log::LogError> = Ok($buf[$off]);
        $off += 1;
        val
    }};
}

pub(crate) use storage_key;

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    storage_key! {
        pub(crate) struct TestKey(40) {
            branch_id: Uuid,
            entity_id: Uuid,
            timestamp_us: i64,
        }
        prefixes {
            prefix_branch(branch_id: Uuid) -> 16,
            prefix_branch_entity(branch_id: Uuid, entity_id: Uuid) -> 32,
        }
    }

    #[test]
    fn roundtrip() {
        let key = TestKey {
            branch_id: Uuid::nil(),
            entity_id: Uuid::from_u128(42),
            timestamp_us: 1_700_000_000_000_000,
        };
        let encoded = key.encode();
        assert_eq!(encoded.len(), 40);

        let decoded = TestKey::decode(&encoded).unwrap();
        assert_eq!(decoded, key);
    }

    #[test]
    fn prefix_branch() {
        let branch = Uuid::nil();
        let prefix = TestKey::prefix_branch(&branch);
        assert_eq!(prefix.len(), 16);
        assert_eq!(&prefix[..], branch.as_bytes());
    }

    #[test]
    fn prefix_branch_entity() {
        let branch = Uuid::nil();
        let entity = Uuid::from_u128(7);
        let prefix = TestKey::prefix_branch_entity(&branch, &entity);
        assert_eq!(prefix.len(), 32);
        assert_eq!(&prefix[..16], branch.as_bytes());
        assert_eq!(&prefix[16..], entity.as_bytes());
    }

    storage_key! {
        pub(crate) struct TestKeyWithU8(33) {
            branch_id: Uuid,
            kind: u8,
            item_id: Uuid,
        }
        prefixes {
            prefix_branch(branch_id: Uuid) -> 16,
            prefix_branch_kind(branch_id: Uuid, kind: u8) -> 17,
        }
    }

    #[test]
    fn u8_roundtrip() {
        let key = TestKeyWithU8 {
            branch_id: Uuid::nil(),
            kind: 42,
            item_id: Uuid::from_u128(7),
        };
        let encoded = key.encode();
        assert_eq!(encoded.len(), 33);
        assert_eq!(encoded[16], 42);

        let decoded = TestKeyWithU8::decode(&encoded).unwrap();
        assert_eq!(decoded, key);
    }

    #[test]
    fn u8_prefix() {
        let branch = Uuid::nil();
        let prefix = TestKeyWithU8::prefix_branch_kind(&branch, &2);
        assert_eq!(prefix.len(), 17);
        assert_eq!(prefix[16], 2);
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
}
