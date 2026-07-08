//! redb-backed [`MultiStore`](crate::store::MultiStore) implementation.

use crate::error::BackendError;
use crate::scan::Direction;
use crate::store::{
    MultiStore, MultiStoreReadHandle, MultiStoreWriteHandle, ReadKVStore, ReadWriteKVStore,
    WriteKVStore,
};
use redb_crate::{
    Database, ReadOnlyTable, ReadTransaction, ReadableDatabase, ReadableTable, Table,
    TableDefinition, WriteTransaction,
};
use std::ops::{Bound, RangeBounds};
use std::path::Path;
use std::sync::Arc;
use std::vec::IntoIter;

const TABLE_PREFIX: &str = "colette:v1";

type BytesTableDefinition<'a> = TableDefinition<'a, &'static [u8], &'static [u8]>;
type ReadTable = ReadOnlyTable<&'static [u8], &'static [u8]>;
type WriteTable<'a> = Table<'a, &'static [u8], &'static [u8]>;
type ScanResult = Result<(Vec<u8>, Vec<u8>), BackendError>;

#[derive(Clone)]
pub struct RedbMultiStore {
    db: Arc<Database>,
}

impl RedbMultiStore {
    pub fn create(path: impl AsRef<Path>) -> Result<Self, BackendError> {
        Ok(Self {
            db: Arc::new(Database::create(path).map_err(BackendError::new)?),
        })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, BackendError> {
        Ok(Self {
            db: Arc::new(Database::open(path).map_err(BackendError::new)?),
        })
    }

    pub fn from_database(db: Database) -> Self {
        Self { db: Arc::new(db) }
    }
}

fn table_definition(name: &str) -> BytesTableDefinition<'_> {
    TableDefinition::new(name)
}

fn collect_scan<T>(
    table: &T,
    range: impl RangeBounds<Vec<u8>>,
    direction: Direction,
) -> Result<IntoIter<ScanResult>, BackendError>
where
    T: ReadableTable<&'static [u8], &'static [u8]>,
{
    let bounds = (
        bytes_bound(range.start_bound()),
        bytes_bound(range.end_bound()),
    );
    let iter = table.range::<&[u8]>(bounds).map_err(BackendError::new)?;
    let results: Vec<ScanResult> = match direction {
        Direction::LeftToRight => iter.map(scan_entry).collect(),
        Direction::RightToLeft => iter.rev().map(scan_entry).collect(),
    };

    Ok(results.into_iter())
}

fn scan_entry(
    entry: redb_crate::Result<(
        redb_crate::AccessGuard<'_, &'static [u8]>,
        redb_crate::AccessGuard<'_, &'static [u8]>,
    )>,
) -> ScanResult {
    let (key, value) = entry.map_err(BackendError::new)?;
    Ok((key.value().to_vec(), value.value().to_vec()))
}

fn bytes_bound(bound: Bound<&Vec<u8>>) -> Bound<&[u8]> {
    match bound {
        Bound::Included(value) => Bound::Included(value.as_slice()),
        Bound::Excluded(value) => Bound::Excluded(value.as_slice()),
        Bound::Unbounded => Bound::Unbounded,
    }
}

fn table_name(namespace: &str, store: &str) -> String {
    let mut name =
        String::with_capacity(TABLE_PREFIX.len() + 2 + namespace.len() * 2 + store.len() * 2);
    name.push_str(TABLE_PREFIX);
    name.push(':');
    push_hex(&mut name, namespace.as_bytes());
    name.push(':');
    push_hex(&mut name, store.as_bytes());
    name
}

fn push_hex(out: &mut String, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_names_are_hex_encoded() {
        assert_eq!(
            table_name("users", "__main"),
            "colette:v1:7573657273:5f5f6d61696e"
        );
    }
}
