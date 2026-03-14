//! Compile-time prefix–entry compatibility marker.

use crate::io::db_item::DbItem;
use crate::io::storage_key::StorageKey;

/// Marker trait: `P` is a valid scan prefix for entry type `T`.
///
/// Prevents passing a wrong prefix to `scan`/`scan_rev` at compile time.
/// Implemented via [`impl_prefix!`] macro.
pub trait ValidPrefix<T: DbItem>: StorageKey {}

/// Declare that a prefix type is valid for one or more entry types.
///
/// ```ignore
/// impl_prefix!(BranchPrefix => EntityEntry, SchemaTypeEntry, TransactionEntry);
/// ```
macro_rules! impl_prefix {
    ($prefix:ty => $( $entry:ty ),+ $(,)?) => {
        $( impl $crate::io::valid_prefix::ValidPrefix<$entry> for $prefix {} )+
    };
}

pub(crate) use impl_prefix;
