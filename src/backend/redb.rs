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
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::ops::{Bound, RangeBounds};
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::vec::IntoIter;

const TABLE_PREFIX: &str = "colette:v1";

type BytesTableDefinition<'a> = TableDefinition<'a, &'static [u8], &'static [u8]>;
type ReadTable = ReadOnlyTable<&'static [u8], &'static [u8]>;
type WriteTable<'a> = Table<'a, &'static [u8], &'static [u8]>;
type ScanResult = Result<(Vec<u8>, Vec<u8>), BackendError>;
type TableKey = (&'static str, &'static str);
type PreparedTables = BTreeMap<TableKey, Arc<str>>;
type SharedPreparedTables = Arc<RwLock<Arc<PreparedTables>>>;

#[derive(Clone)]
pub struct RedbMultiStore {
    db: Arc<Database>,
    tables: SharedPreparedTables,
}

impl RedbMultiStore {
    pub fn create(path: impl AsRef<Path>) -> Result<Self, BackendError> {
        Ok(Self::from_database(
            Database::create(path).map_err(BackendError::new)?,
        ))
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, BackendError> {
        Ok(Self::from_database(
            Database::open(path).map_err(BackendError::new)?,
        ))
    }

    pub fn from_database(db: Database) -> Self {
        Self {
            db: Arc::new(db),
            tables: Arc::new(RwLock::new(Arc::new(PreparedTables::new()))),
        }
    }
}

impl MultiStore for RedbMultiStore {
    type ReadHandle = RedbReadHandle;
    type WriteHandle = RedbWriteHandle;

    fn prepare(
        &self,
        namespace: &'static str,
        stores: impl IntoIterator<Item = &'static str>,
    ) -> Result<(), BackendError> {
        let prepared = stores
            .into_iter()
            .map(|store| {
                (
                    (namespace, store),
                    Arc::<str>::from(table_name(namespace, store)),
                )
            })
            .collect::<Vec<_>>();
        let write = self.db.begin_write().map_err(BackendError::new)?;

        for (_, name) in &prepared {
            let table = write
                .open_table(table_definition(name.as_ref()))
                .map_err(BackendError::new)?;
            drop(table);
        }

        write.commit().map_err(BackendError::new)?;

        let mut tables = self.tables.write().unwrap();
        let mut next = tables.as_ref().clone();
        next.retain(|(prepared_namespace, _), _| *prepared_namespace != namespace);
        for (key, name) in prepared {
            next.insert(key, name);
        }
        *tables = Arc::new(next);

        Ok(())
    }

    fn read(&self, namespace: &'static str) -> Result<Self::ReadHandle, BackendError> {
        Ok(RedbReadHandle {
            namespace,
            tables: self.tables.read().unwrap().clone(),
            read: self.db.begin_read().map_err(BackendError::new)?,
        })
    }

    fn write(&self, namespace: &'static str) -> Result<Self::WriteHandle, BackendError> {
        Ok(RedbWriteHandle {
            namespace,
            tables: self.tables.read().unwrap().clone(),
            write: self.db.begin_write().map_err(BackendError::new)?,
        })
    }
}

pub struct RedbReadHandle {
    namespace: &'static str,
    tables: Arc<PreparedTables>,
    read: ReadTransaction,
}

impl MultiStoreReadHandle for RedbReadHandle {
    type Store = RedbReadStore;

    fn open_store(&self, name: &'static str) -> Result<Self::Store, BackendError> {
        let table = prepared_table_name(&self.tables, self.namespace, name)?;
        Ok(RedbReadStore {
            table: self
                .read
                .open_table(table_definition(table.as_ref()))
                .map_err(BackendError::new)?,
        })
    }
}

pub struct RedbWriteHandle {
    namespace: &'static str,
    tables: Arc<PreparedTables>,
    write: WriteTransaction,
}

impl MultiStoreWriteHandle for RedbWriteHandle {
    type Store<'a> = RedbWriteStore<'a>;

    fn open_store(&mut self, name: &'static str) -> Result<Self::Store<'_>, BackendError> {
        let table = prepared_table_name(&self.tables, self.namespace, name)?;
        Ok(RedbWriteStore {
            table: self
                .write
                .open_table(table_definition(table.as_ref()))
                .map_err(BackendError::new)?,
        })
    }

    fn commit(self) -> Result<(), BackendError> {
        self.write.commit().map_err(BackendError::new)
    }
}

pub struct RedbReadStore {
    table: ReadTable,
}

impl ReadKVStore for RedbReadStore {
    type Iter = IntoIter<ScanResult>;

    fn get(&self, key: impl AsRef<[u8]>) -> Result<Option<Vec<u8>>, BackendError> {
        self.table
            .get(key.as_ref())
            .map(|value| value.map(|value| value.value().to_vec()))
            .map_err(BackendError::new)
    }

    fn scan(
        self,
        range: impl RangeBounds<Vec<u8>>,
        direction: Direction,
    ) -> Result<Self::Iter, BackendError> {
        collect_scan(&self.table, range, direction)
    }
}

pub struct RedbWriteStore<'a> {
    table: WriteTable<'a>,
}

impl<'a> ReadWriteKVStore<'a> for RedbWriteStore<'a> {}

impl<'a> WriteKVStore<'a> for RedbWriteStore<'a> {
    fn set(&mut self, key: impl AsRef<[u8]>, value: impl AsRef<[u8]>) -> Result<(), BackendError> {
        self.table
            .insert(key.as_ref(), value.as_ref())
            .map(|_| ())
            .map_err(BackendError::new)
    }

    fn remove(&mut self, key: impl AsRef<[u8]>) -> Result<(), BackendError> {
        self.table
            .remove(key.as_ref())
            .map(|_| ())
            .map_err(BackendError::new)
    }
}

impl ReadKVStore for RedbWriteStore<'_> {
    type Iter = IntoIter<ScanResult>;

    fn get(&self, key: impl AsRef<[u8]>) -> Result<Option<Vec<u8>>, BackendError> {
        self.table
            .get(key.as_ref())
            .map(|value| value.map(|value| value.value().to_vec()))
            .map_err(BackendError::new)
    }

    fn scan(
        self,
        range: impl RangeBounds<Vec<u8>>,
        direction: Direction,
    ) -> Result<Self::Iter, BackendError> {
        collect_scan(&self.table, range, direction)
    }
}

fn table_definition(name: &str) -> BytesTableDefinition<'_> {
    TableDefinition::new(name)
}

fn prepared_table_name(
    tables: &PreparedTables,
    namespace: &'static str,
    store: &'static str,
) -> Result<Arc<str>, BackendError> {
    tables
        .get(&(namespace, store))
        .cloned()
        .ok_or_else(|| BackendError::new(UnpreparedStore { namespace, store }))
}

#[derive(Debug)]
struct UnpreparedStore {
    namespace: &'static str,
    store: &'static str,
}

impl Display for UnpreparedStore {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "store '{}' in namespace '{}' has not been prepared",
            self.store, self.namespace
        )
    }
}

impl std::error::Error for UnpreparedStore {}

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
    use crate::backend::tests::multistore_contract_tests;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DB: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn table_names_are_hex_encoded() {
        assert_eq!(
            table_name("users", "__main"),
            "colette:v1:7573657273:5f5f6d61696e"
        );
    }

    #[test]
    fn write_open_store_rejects_unprepared_stores() {
        let db = make_db();
        db.prepare("users", ["__main"]).unwrap();

        let mut write = db.write("users").unwrap();

        assert!(write.open_store("missing").is_err());
    }

    #[test]
    fn prepare_replaces_namespace_registry_snapshot() {
        let db = make_db();
        db.prepare("users", ["old"]).unwrap();

        let read_before = db.read("users").unwrap();

        db.prepare("users", ["new"]).unwrap();

        assert!(read_before.open_store("old").is_ok());
        assert!(read_before.open_store("new").is_err());

        let read_after = db.read("users").unwrap();
        assert!(read_after.open_store("old").is_err());
        assert!(read_after.open_store("new").is_ok());
    }

    fn make_db() -> RedbMultiStore {
        RedbMultiStore::create(temp_db_path()).unwrap()
    }

    fn temp_db_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "colette-redb-contract-{}-{}.redb",
            std::process::id(),
            NEXT_DB.fetch_add(1, Ordering::Relaxed)
        ))
    }

    multistore_contract_tests!(make_db);
}
