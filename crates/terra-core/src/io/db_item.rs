//! Trait for types that know how to store themselves in the database.

use super::storage_key::StorageKey;
use super::storage_value::StorageValue;

/// Contract between domain types and storage.
///
/// A `DbItem` is a composite of a typed key and a typed value.
/// Storage layer uses these associated types for type-safe get/put.
pub trait DbItem: Sized {
    /// Persisted codec version for this column family. Version 1 denotes legacy unmarked data.
    const FORMAT_VERSION: u32 = 1;
    type Key: StorageKey;
    type Value: StorageValue;

    /// Column family this item belongs to.
    fn cf() -> &'static str;

    /// Access the key.
    fn key(&self) -> &Self::Key;

    /// Access the value.
    fn value(&self) -> &Self::Value;

    /// Reconstruct from decoded key and value.
    fn from_parts(key: Self::Key, value: Self::Value) -> Self;
}
