//! Iterator types produced by collection and index scans.

use crate::error::Error;
use crate::inline_vec::IVec;
use crate::item::Item;
use crate::key::Key;
use crate::store::{KVEntry, ReadKVStore};
use std::marker::PhantomData;
use std::ops::Deref;

/// One record returned from a collection or index scan.
pub struct Entry<Record> {
    /// The decoded record.
    pub record: Record,
    /// Cursor for resuming a scan after this entry.
    pub key: Cursor,
}

impl<Record: Item> AsRef<Record> for Entry<Record> {
    fn as_ref(&self) -> &Record {
        &self.record
    }
}

impl<Record: Item> Deref for Entry<Record> {
    type Target = Record;

    fn deref(&self) -> &Self::Target {
        &self.record
    }
}

/// Opaque cursor for resuming a scan.
///
/// Pass a cursor to `after` on a scan builder to resume after the corresponding
/// entry. Cursors yielded by iterators are already encoded for the scan that
/// produced them.
///
/// [`Cursor::None`] represents the absence of a cursor. This is useful for
/// cursor-based pagination APIs where the first page has no resume point yet.
///
/// For primary collection scans, build a cursor from a primary key with
/// [`Cursor::from_key`]. For secondary index scans, prefer
/// [`Index::cursor`](crate::Index::cursor), which accounts for the index kind's
/// physical key layout.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Cursor {
    /// No cursor.
    ///
    /// Passing this to `after` leaves the scan unchanged.
    #[default]
    None,
    /// Encoded cursor key.
    ///
    /// Values yielded by collection and index iterators use this variant.
    Key(IVec),
}

impl Cursor {
    /// Builds a cursor from an ordered key.
    ///
    /// This is primarily useful for collection scans, whose cursor layout is
    /// the record primary key.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let cursor = maybe_cursor.unwrap_or_default();
    ///
    /// let page = users.scan()?
    ///     .after(cursor)
    ///     .iter()?;
    /// ```
    pub fn from_key(key: impl Key) -> Self {
        Self::Key(IVec::from(key.encode().as_ref()))
    }

    pub(crate) fn into_key_vec(self) -> Option<Vec<u8>> {
        match self {
            Self::None => None,
            Self::Key(key) => Some(key.into_vec()),
        }
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
    type Item = Result<Entry<Record>, Error>;

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

                Ok(Entry {
                    record,
                    key: Cursor::Key(IVec::from(entry.key())),
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
    type Item = Result<Entry<Record>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|res| {
            res.map_err(Error::backend).and_then(|entry| {
                let record = Record::from_bytes(entry.value()).map_err(Error::codec)?;

                Ok(Entry {
                    record,
                    key: Cursor::Key(IVec::from(entry.key())),
                })
            })
        })
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn cursor_from_key_encodes_primary_key_bytes() {
        assert_eq!(
            Cursor::from_key(42u64).into_key_vec().unwrap(),
            42u64.encode().as_ref()
        );
    }

    #[test]
    fn cursor_from_key_accepts_composite_keys() {
        assert_eq!(
            Cursor::from_key(("core", 7u64)).into_key_vec().unwrap(),
            ("core", 7u64).encode().as_ref()
        );
    }

    #[test]
    fn cursor_default_is_none() {
        assert_eq!(Cursor::default(), Cursor::None);
        assert_eq!(Cursor::None.into_key_vec(), None);
    }
}
