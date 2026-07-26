//! Iterator types produced by collection and index scans.

use crate::error::Error;
use crate::inline_vec::IVec;
use crate::item::Item;
use crate::key::Key;
use crate::store::{KVEntry, ReadKVStore};
use std::marker::PhantomData;

/// One record returned from a collection or index scan.
pub struct IndexEntry<Record> {
    /// The decoded record.
    pub record: Record,
    /// Cursor for resuming a scan after this entry.
    pub key: Cursor,
}

/// Opaque cursor key for a scan entry.
///
/// Pass a cursor to `after` on a scan builder to resume after the corresponding
/// entry. Cursors yielded by iterators are already encoded for the scan that
/// produced them.
///
/// For primary collection scans, build a cursor from a primary key with
/// [`Cursor::from_key`]. For secondary index scans, prefer
/// [`Index::cursor`](crate::Index::cursor), which accounts for the index kind's
/// physical key layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cursor(IVec);

impl Cursor {
    /// Builds a cursor from an ordered key.
    ///
    /// This is primarily useful for collection scans, whose cursor layout is
    /// the record primary key.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let next_page = users.scan()?
    ///     .after(Cursor::from_key(42u64))
    ///     .iter()?;
    /// ```
    pub fn from_key(key: impl Key) -> Self {
        Self(IVec::from(key.encode().as_ref()))
    }

    pub(crate) fn into_vec(self) -> Vec<u8> {
        self.0.into_vec()
    }
}

/// Iterator over records matched by a secondary index scan.
///
/// Each index entry stores a primary key; the iterator follows that key into the
/// collection's primary store and decodes the record before yielding it.
pub struct IndexIterator<Store, Record>
where
    Store: ReadKVStore,
    Record: Item,
{
    inner: Store::Iter,
    primary_store: Store,

    _marker: PhantomData<Record>,
}

impl<Store, Record> IndexIterator<Store, Record>
where
    Store: ReadKVStore,
    Record: Item,
{
    /// Creates an iterator from an index-store iterator and the primary store.
    pub fn new(inner: Store::Iter, primary_store: Store) -> Self {
        Self {
            inner,
            primary_store,

            _marker: PhantomData,
        }
    }
}

impl<Store, Record> Iterator for IndexIterator<Store, Record>
where
    Store: ReadKVStore,
    Record: Item,
{
    type Item = Result<IndexEntry<Record>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|res| {
            res.map_err(Error::backend).and_then(|entry| {
                let record_bytes = self
                    .primary_store
                    .get(entry.value())
                    .map_err(Error::backend)?
                    .ok_or(Error::Unexpected(format!(
                        "primary key from index not found: {:?}",
                        entry.value()
                    )))?;
                let record = Record::from_bytes(record_bytes.as_ref()).map_err(Error::codec)?;

                Ok(IndexEntry {
                    record,
                    key: Cursor(IVec::from(entry.key())),
                })
            })
        })
    }
}

pub struct CollectionIterator<Store, Record>
where
    Store: ReadKVStore,
    Record: Item,
{
    inner: Store::Iter,

    _marker: PhantomData<Record>,
}

impl<Store, Record> CollectionIterator<Store, Record>
where
    Store: ReadKVStore,
    Record: Item,
{
    /// Creates an iterator from a primary-store iterator.
    pub fn new(inner: Store::Iter) -> Self {
        Self {
            inner,

            _marker: PhantomData,
        }
    }
}

impl<Store, Record> Iterator for CollectionIterator<Store, Record>
where
    Store: ReadKVStore,
    Record: Item,
{
    type Item = Result<IndexEntry<Record>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|res| {
            res.map_err(Error::backend).and_then(|entry| {
                let record = Record::from_bytes(entry.value()).map_err(Error::codec)?;

                Ok(IndexEntry {
                    record,
                    key: Cursor(IVec::from(entry.key())),
                })
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_from_key_encodes_primary_key_bytes() {
        assert_eq!(Cursor::from_key(42u64).into_vec(), 42u64.encode().as_ref());
    }

    #[test]
    fn cursor_from_key_accepts_composite_keys() {
        assert_eq!(
            Cursor::from_key(("core", 7u64)).into_vec(),
            ("core", 7u64).encode().as_ref()
        );
    }
}
