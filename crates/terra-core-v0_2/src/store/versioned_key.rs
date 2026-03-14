//! Trait and macro for keys of records that live inside transactions.
//!
//! All versioned entries follow the pattern `branch_id | ... | tx_id` —
//! branch first (for ancestry walk), tx last (for reverse scan to latest).
//! The middle is domain-specific addressing (all fields Uuid).

use uuid::Uuid;

use crate::io::storage_key::StorageKey;

/// Key of a versioned, branch-scoped record.
///
/// Implemented by keys whose entries are created inside transactions
/// and follow the `branch_id | domain | tx_id` layout.
pub trait VersionedKey: StorageKey {
    fn branch_id(&self) -> Uuid;
    fn tx_id(&self) -> Uuid;
}

/// Declares a versioned storage key: `branch_id(16) | middle Uuid fields | tx_id(16)`.
///
/// Generates the struct, `StorageKey` impl, and `VersionedKey` impl.
/// Only the middle domain fields need to be specified (all Uuid).
/// Size is computed automatically.
///
/// ```ignore
/// versioned_key! {
///     pub struct EntityKey {
///         entity_id: Uuid,
///     }
/// }
/// // Expands to a 48-byte key: branch_id(16) | entity_id(16) | tx_id(16)
/// ```
macro_rules! versioned_key {
    (
        $vis:vis struct $name:ident {
            $( $field:ident : Uuid ),* $(,)?
        }
    ) => {
        #[derive(Debug, Clone, PartialEq, Eq)]
        $vis struct $name {
            pub branch_id: uuid::Uuid,
            $( pub $field: uuid::Uuid, )*
            pub tx_id: uuid::Uuid,
        }

        impl $crate::io::storage_key::StorageKey for $name {
            const SIZE: usize = (2 + versioned_key!(@count $( $field )*)) * 16;

            fn encode(&self) -> Vec<u8> {
                let mut buf = vec![0u8; Self::SIZE];
                let mut _off: usize = 0;

                buf[_off.._off + 16].copy_from_slice(self.branch_id.as_bytes());
                _off += 16;
                $({
                    buf[_off.._off + 16].copy_from_slice(self.$field.as_bytes());
                    _off += 16;
                })*
                buf[_off.._off + 16].copy_from_slice(self.tx_id.as_bytes());

                buf
            }

            fn decode(bytes: &[u8]) -> Result<Self, $crate::io::storage_key::KeyError> {
                if bytes.len() < Self::SIZE {
                    return Err($crate::io::storage_key::KeyError(
                        format!("{} key too short: {} < {}", stringify!($name), bytes.len(), Self::SIZE)
                    ));
                }
                let mut _off: usize = 0;

                let branch_id = uuid::Uuid::from_slice(&bytes[_off.._off + 16])
                    .map_err(|e| $crate::io::storage_key::KeyError(e.to_string()))?;
                _off += 16;
                $(
                    let $field = uuid::Uuid::from_slice(&bytes[_off.._off + 16])
                        .map_err(|e| $crate::io::storage_key::KeyError(e.to_string()))?;
                    _off += 16;
                )*
                let tx_id = uuid::Uuid::from_slice(&bytes[_off.._off + 16])
                    .map_err(|e| $crate::io::storage_key::KeyError(e.to_string()))?;

                Ok(Self { branch_id, $( $field, )* tx_id })
            }
        }

        impl $crate::store::versioned_key::VersionedKey for $name {
            fn branch_id(&self) -> uuid::Uuid { self.branch_id }
            fn tx_id(&self) -> uuid::Uuid { self.tx_id }
        }
    };

    (@count) => { 0usize };
    (@count $head:ident $( $tail:ident )*) => { 1usize + versioned_key!(@count $( $tail )*) };
}

pub(crate) use versioned_key;

#[cfg(test)]
mod tests {
    use super::*;

    versioned_key! {
        pub struct TwoFieldKey {
            entity_id: Uuid,
        }
    }

    versioned_key! {
        pub struct EmptyMiddleKey {}
    }

    versioned_key! {
        pub struct ThreeFieldKey {
            a: Uuid,
            b: Uuid,
            c: Uuid,
        }
    }

    #[test]
    fn auto_size() {
        assert_eq!(EmptyMiddleKey::SIZE, 32);
        assert_eq!(TwoFieldKey::SIZE, 48);
        assert_eq!(ThreeFieldKey::SIZE, 80);
    }

    #[test]
    fn roundtrip_with_middle() {
        let key = TwoFieldKey {
            branch_id: Uuid::from_u128(1),
            entity_id: Uuid::from_u128(2),
            tx_id: Uuid::from_u128(3),
        };
        let bytes = key.encode();
        assert_eq!(bytes.len(), 48);
        let decoded = TwoFieldKey::decode(&bytes).unwrap();
        assert_eq!(decoded, key);
    }

    #[test]
    fn roundtrip_empty_middle() {
        let key = EmptyMiddleKey {
            branch_id: Uuid::from_u128(1),
            tx_id: Uuid::from_u128(2),
        };
        let bytes = key.encode();
        assert_eq!(bytes.len(), 32);
        let decoded = EmptyMiddleKey::decode(&bytes).unwrap();
        assert_eq!(decoded, key);
    }

    #[test]
    fn versioned_key_trait() {
        let key = TwoFieldKey {
            branch_id: Uuid::from_u128(10),
            entity_id: Uuid::from_u128(20),
            tx_id: Uuid::from_u128(30),
        };
        assert_eq!(key.branch_id(), Uuid::from_u128(10));
        assert_eq!(key.tx_id(), Uuid::from_u128(30));
    }

    #[test]
    fn keys_sort_by_fields_then_tx() {
        let k1 = TwoFieldKey {
            branch_id: Uuid::from_u128(1),
            entity_id: Uuid::from_u128(2),
            tx_id: Uuid::from_u128(10),
        };
        let k2 = TwoFieldKey {
            branch_id: Uuid::from_u128(1),
            entity_id: Uuid::from_u128(2),
            tx_id: Uuid::from_u128(20),
        };
        assert!(k1.encode() < k2.encode());
    }
}
