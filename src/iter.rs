//! Iterator types produced by index scans.

use crate::error::Error;
use crate::item::Item;
use crate::store::{KVEntry, ReadKVStore};
use std::marker::PhantomData;

/// One record returned from an index scan.
pub struct IndexEntry<Record> {
    /// The decoded record loaded from the collection primary store.
    pub record: Record,
    /// Cursor for resuming a scan after this entry.
    pub key: Cursor,
}

/// Opaque cursor key for an index entry.
///
/// Cursor support is currently internal-facing; future APIs may expose stable
/// cursor serialization.
#[allow(dead_code)]
pub struct Cursor(Vec<u8>);

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
                    key: Cursor(entry.key().to_vec()),
                })
            })
        })
    }
}
