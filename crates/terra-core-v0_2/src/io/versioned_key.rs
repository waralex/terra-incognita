//! Trait and macro for keys of records that live inside transactions.
//!
//! All versioned entries follow the pattern `branch_id | ... | tx_id` —
//! branch first (for ancestry walk), tx last (for reverse scan to latest).
//! The middle is domain-specific addressing.

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
    // 0 middle fields: 32 bytes
    ( $vis:vis struct $name:ident {} ) => {
        versioned_key!(@impl $vis, $name, 32, { branch_id tx_id }, []);
    };
    // 1 middle field: 48 bytes
    ( $vis:vis struct $name:ident { $f1:ident : Uuid $(,)? } ) => {
        versioned_key!(@impl $vis, $name, 48, { branch_id $f1 tx_id }, [$f1]);
    };
    // 2 middle fields: 64 bytes
    ( $vis:vis struct $name:ident { $f1:ident : Uuid, $f2:ident : Uuid $(,)? } ) => {
        versioned_key!(@impl $vis, $name, 64, { branch_id $f1 $f2 tx_id }, [$f1 $f2]);
    };
    // 3 middle fields: 80 bytes
    ( $vis:vis struct $name:ident { $f1:ident : Uuid, $f2:ident : Uuid, $f3:ident : Uuid $(,)? } ) => {
        versioned_key!(@impl $vis, $name, 80, { branch_id $f1 $f2 $f3 tx_id }, [$f1 $f2 $f3]);
    };

    // Internal: generate struct + StorageKey + VersionedKey.
    (@impl $vis:vis, $name:ident, $size:literal, { $( $field:ident )+ }, [ $( $mid:ident )* ]) => {
        #[derive(Debug, Clone, PartialEq, Eq)]
        $vis struct $name {
            $( pub $field: uuid::Uuid ),+
        }

        impl $crate::io::storage_key::StorageKey for $name {
            const SIZE: usize = $size;

            fn encode(&self) -> Vec<u8> {
                let mut buf = vec![0u8; $size];
                let mut _off: usize = 0;
                $({
                    buf[_off.._off + 16].copy_from_slice(self.$field.as_bytes());
                    _off += 16;
                })+
                buf
            }

            fn decode(bytes: &[u8]) -> Result<Self, $crate::io::storage_key::KeyError> {
                if bytes.len() < $size {
                    return Err($crate::io::storage_key::KeyError(
                        format!("{} key too short: {} < {}", stringify!($name), bytes.len(), $size)
                    ));
                }
                let mut _off: usize = 0;
                $(
                    let $field = uuid::Uuid::from_slice(&bytes[_off.._off + 16])
                        .map_err(|e| $crate::io::storage_key::KeyError(e.to_string()))?;
                    _off += 16;
                )+
                Ok(Self { $( $field ),+ })
            }
        }

        impl $crate::io::versioned_key::VersionedKey for $name {
            fn branch_id(&self) -> uuid::Uuid { self.branch_id }
            fn tx_id(&self) -> uuid::Uuid { self.tx_id }
        }
    };
}

pub(crate) use versioned_key;
