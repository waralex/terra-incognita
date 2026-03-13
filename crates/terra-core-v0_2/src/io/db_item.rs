//! Trait for types that know how to store themselves in the database.

use super::terra_db::DbError;

/// Contract between domain types and storage.
///
/// A `DbItem` knows its column family, how to serialize its key and value,
/// and how to reconstruct itself from bytes. Storage layer calls these
/// methods — it never manipulates raw bytes directly.
pub trait DbItem: Sized {
    /// Column family this item belongs to.
    fn cf() -> &'static str;

    /// Encode the storage key.
    fn encode_key(&self) -> Vec<u8>;

    /// Encode the storage value.
    fn encode_value(&self) -> Result<Vec<u8>, DbError>;

    /// Reconstruct from raw key and value bytes.
    fn decode(key: &[u8], value: &[u8]) -> Result<Self, DbError>;
}
