//! Typed iterator over database entries within a key range.

use std::marker::PhantomData;

use rocksdb::{DBIteratorWithThreadMode, Direction, IteratorMode, DB};

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
        self.inner
            .set_mode(IteratorMode::From(&point, self.direction));
    }
}

impl<T: DbItem> Iterator for DbIterator<'_, T> {
    type Item = Result<T, DbError>;

    fn next(&mut self) -> Option<Self::Item> {
        let (key_bytes, val_bytes) = match self.inner.next()? {
            Ok(kv) => kv,
            Err(e) => return Some(Err(DbError::Storage(e.to_string()))),
        };
        if key_bytes.as_ref() < self.lower.as_slice() || key_bytes.as_ref() > self.upper.as_slice()
        {
            return None;
        }
        let key = match T::Key::decode(&key_bytes) {
            Ok(k) => k,
            Err(e) => return Some(Err(e.into())),
        };
        #[cfg(test)]
        VALUE_READS.with(|n| n.set(n.get() + 1));
        let value = match T::Value::decode(&val_bytes) {
            Ok(v) => v,
            Err(e) => return Some(Err(e)),
        };
        Some(Ok(T::from_parts(key, value)))
    }
}

#[cfg(test)]
thread_local! {
    pub(crate) static VALUE_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(crate) static KEY_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Forward key-only scan. Does not copy or deserialize stored values.
pub struct DbKeyIterator<'a, T: DbItem> {
    inner: rocksdb::DBRawIteratorWithThreadMode<'a, DB>,
    lower: Vec<u8>,
    upper: Vec<u8>,
    advance: bool,
    _marker: PhantomData<T>,
}
impl<'a, T: DbItem> DbKeyIterator<'a, T> {
    pub(super) fn new(
        mut inner: rocksdb::DBRawIteratorWithThreadMode<'a, DB>,
        lower: Vec<u8>,
        upper: Vec<u8>,
    ) -> Self {
        inner.seek(&lower);
        Self {
            inner,
            lower,
            upper,
            advance: false,
            _marker: PhantomData,
        }
    }
    pub fn seek(&mut self, prefix: &impl KeyPrefix<Key = T::Key>) {
        self.inner.seek(prefix.encode_lower_bound());
        self.advance = false;
    }
}
impl<T: DbItem> Iterator for DbKeyIterator<'_, T> {
    type Item = Result<T::Key, DbError>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.advance {
            self.inner.next();
        }
        self.advance = false;
        let Some(bytes) = self.inner.key() else {
            return self
                .inner
                .status()
                .err()
                .map(|e| Err(DbError::Storage(e.to_string())));
        };
        if bytes < self.lower.as_slice() || bytes > self.upper.as_slice() {
            return None;
        }
        self.advance = true;
        #[cfg(test)]
        KEY_READS.with(|n| n.set(n.get() + 1));
        Some(T::Key::decode(bytes).map_err(Into::into))
    }
}
