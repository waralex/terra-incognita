//! Compile-time prefix–key compatibility marker.

use crate::io::key_prefix::KeyPrefix;
use crate::io::storage_key::StorageKey;

/// Marker trait: `P` is a valid scan prefix for key type `K`.
///
/// Prevents passing a wrong prefix to `scan`/`scan_rev` at compile time.
/// Implemented via [`impl_prefix!`] macro.
pub trait ValidPrefix<K: StorageKey>: KeyPrefix {}

/// Declare that a prefix type is valid for one or more key types.
///
/// ```ignore
/// impl_prefix!(EntityKeyPrefix => EntityKey);
/// ```
macro_rules! impl_prefix {
    ($prefix:ty => $( $key:ty ),+ $(,)?) => {
        $( impl $crate::io::valid_prefix::ValidPrefix<$key> for $prefix {} )+
    };
}

pub(crate) use impl_prefix;
