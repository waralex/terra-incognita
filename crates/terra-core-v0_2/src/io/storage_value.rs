//! Trait for encoding/decoding storage values.

use super::terra_db::DbError;

/// Contract for value serialization.
///
/// Each value type controls its own encoding — JSON, binary, etc.
pub trait StorageValue: Sized {
    fn encode(&self) -> Result<Vec<u8>, DbError>;
    fn decode(bytes: &[u8]) -> Result<Self, DbError>;
}
