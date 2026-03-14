//! Typed iterator over database entries matching a key prefix.

use std::marker::PhantomData;

use rocksdb::{DBIteratorWithThreadMode, DB};

use crate::io::db_item::DbItem;
use crate::io::storage_key::StorageKey;
use crate::io::storage_value::StorageValue;
use crate::io::terra_db::DbError;

/// Typed iterator over [`DbItem`] entries sharing a common key prefix.
///
/// Decodes key + value on each step. Stops when the underlying key
/// no longer starts with the prefix used to create the iterator.
pub struct DbIterator<'a, T: DbItem> {
    inner: DBIteratorWithThreadMode<'a, DB>,
    prefix: Vec<u8>,
    _marker: PhantomData<T>,
}

impl<'a, T: DbItem> DbIterator<'a, T> {
    pub(super) fn new(
        inner: DBIteratorWithThreadMode<'a, DB>,
        prefix: Vec<u8>,
    ) -> Self {
        Self {
            inner,
            prefix,
            _marker: PhantomData,
        }
    }
}

impl<T: DbItem> Iterator for DbIterator<'_, T> {
    type Item = Result<T, DbError>;

    fn next(&mut self) -> Option<Self::Item> {
        let (key_bytes, val_bytes) = match self.inner.next()? {
            Ok(kv) => kv,
            Err(e) => return Some(Err(DbError::Storage(e.to_string()))),
        };
        if !key_bytes.starts_with(&self.prefix) {
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
