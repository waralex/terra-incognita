//! Typed iterator over database entries within a key range.

use std::marker::PhantomData;

use rocksdb::{DBIteratorWithThreadMode, DB, Direction, IteratorMode};

use crate::io::db_item::DbItem;
use crate::io::key_prefix::KeyPrefix;
use crate::io::storage_key::StorageKey;
use crate::io::storage_value::StorageValue;
use crate::io::terra_db::DbError;

/// Typed iterator over [`DbItem`] entries within `lower..=upper` key range.
///
/// Decodes key + value on each step. Stops when the underlying key
/// falls outside the `[lower, upper]` bounds.
pub struct DbIterator<'a, T: DbItem> {
    inner: DBIteratorWithThreadMode<'a, DB>,
    lower: Vec<u8>,
    upper: Vec<u8>,
    direction: Direction,
    _marker: PhantomData<T>,
}

impl<'a, T: DbItem> DbIterator<'a, T> {
    pub(super) fn new(
        inner: DBIteratorWithThreadMode<'a, DB>,
        lower: Vec<u8>,
        upper: Vec<u8>,
        direction: Direction,
    ) -> Self {
        Self {
            inner,
            lower,
            upper,
            direction,
            _marker: PhantomData,
        }
    }

    /// Reposition the iterator to the given prefix.
    ///
    /// Uses `encode_lower_bound` for forward iterators and
    /// `encode_upper_bound` for reverse iterators.
    pub fn seek(&mut self, prefix: &impl KeyPrefix<Key = T::Key>) {
        let point = match self.direction {
            Direction::Forward => prefix.encode_lower_bound(),
            Direction::Reverse => prefix.encode_upper_bound(),
        };
        self.inner.set_mode(IteratorMode::From(&point, self.direction));
    }
}

impl<T: DbItem> Iterator for DbIterator<'_, T> {
    type Item = Result<T, DbError>;

    fn next(&mut self) -> Option<Self::Item> {
        let (key_bytes, val_bytes) = match self.inner.next()? {
            Ok(kv) => kv,
            Err(e) => return Some(Err(DbError::Storage(e.to_string()))),
        };
        if key_bytes.as_ref() < self.lower.as_slice()
            || key_bytes.as_ref() > self.upper.as_slice()
        {
            return None;
        }
        let key = match T::Key::decode(&key_bytes) {
            Ok(k) => k,
            Err(e) => return Some(Err(e.into())),
        };
        let value = match T::Value::decode(&val_bytes) {
            Ok(v) => v,
            Err(e) => return Some(Err(e)),
        };
        Some(Ok(T::from_parts(key, value)))
    }
}
