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

fn table_name(namespace: &str, store: &str) -> String {
    let mut name = String::with_capacity(TABLE_PREFIX.len() + 2 + namespace.len() * 2 + store.len() * 2);
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
