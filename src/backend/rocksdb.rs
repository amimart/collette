//! Persistent [`rocksdb`](https://crates.io/crates/rocksdb)-backed
//! [`MultiStore`](crate::store::MultiStore) implementation.
//!
//! Enable the `rocksdb` feature to use this backend.

use crate::error::BackendError;
use crate::scan::Direction;
use crate::store::{
    KVEntry, MultiStore, MultiStoreReadHandle, MultiStoreWriteHandle, ReadKVStore,
    ReadWriteKVStore, WriteKVStore,
};
use ::rocksdb::{
    Direction as RocksDirection, Error as RocksError, IteratorMode, Options, ReadOptions,
    SnapshotWithThreadMode, Transaction, TransactionDB, TransactionDBOptions, TransactionOptions,
    WriteOptions,
};
use std::collections::BTreeMap;
use std::ops::{Bound, RangeBounds};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, RwLock};

type TableKey = (&'static str, &'static str);
type PreparedStores = BTreeMap<TableKey, Arc<[u8]>>;
type SharedPreparedStores = Arc<RwLock<Arc<PreparedStores>>>;
type NamespaceWriters = BTreeMap<&'static str, Arc<WriterGate>>;
type SharedNamespaceWriters = Arc<Mutex<NamespaceWriters>>;
type RocksSnapshot = SnapshotWithThreadMode<'static, TransactionDB>;
type RocksTransaction = Transaction<'static, TransactionDB>;
type ScanResult = Result<RocksDbEntry, BackendError>;

/// Persistent Collette backend backed by RocksDB.
///
/// Collette namespaces and stores are encoded as key prefixes in one RocksDB
/// keyspace. This avoids dynamic column-family lifecycle management while still
/// preserving logical namespace and store isolation.
#[derive(Clone)]
pub struct RocksDbMultiStore {
    db: Arc<TransactionDB>,
    stores: SharedPreparedStores,
    writers: SharedNamespaceWriters,
}

impl RocksDbMultiStore {
    /// Creates a new RocksDB database at `path`.
    pub fn create(path: impl AsRef<Path>) -> Result<Self, BackendError> {
        let mut opts = default_options();
        opts.set_error_if_exists(true);

        Self::open_with_options(path, &opts, &TransactionDBOptions::default())
    }

    /// Opens a RocksDB database at `path`, creating it if it does not exist.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, BackendError> {
        Self::open_with_options(path, &default_options(), &TransactionDBOptions::default())
    }

    /// Opens a RocksDB database with explicit RocksDB options.
    pub fn open_with_options(
        path: impl AsRef<Path>,
        opts: &Options,
        txn_opts: &TransactionDBOptions,
    ) -> Result<Self, BackendError> {
        Self::from_database(TransactionDB::open(opts, txn_opts, path).map_err(BackendError::new)?)
    }

    /// Wraps an existing RocksDB [`TransactionDB`].
    pub fn from_database(db: TransactionDB) -> Result<Self, BackendError> {
        Ok(Self {
            db: Arc::new(db),
            stores: Arc::new(RwLock::new(Arc::new(PreparedStores::new()))),
            writers: Arc::new(Mutex::new(NamespaceWriters::new())),
        })
    }
}

impl MultiStore for RocksDbMultiStore {
    type Error = BackendError;
    type ReadHandle = RocksDbReadHandle;
    type WriteHandle = RocksDbWriteHandle;

    fn prepare(
        &self,
        namespace: &'static str,
        stores: impl IntoIterator<Item = &'static str>,
    ) -> Result<(), Self::Error> {
        let prepared = stores
            .into_iter()
            .map(|store| {
                (
                    (namespace, store),
                    Arc::<[u8]>::from(store_prefix(namespace, store)),
                )
            })
            .collect::<Vec<_>>();

        let mut stores = self.stores.write().unwrap();
        let mut next = stores.as_ref().clone();
        next.retain(|(prepared_namespace, _), _| *prepared_namespace != namespace);
        for (key, prefix) in prepared {
            next.insert(key, prefix);
        }
        *stores = Arc::new(next);

        let mut writers = self.writers.lock().unwrap();
        writers
            .entry(namespace)
            .or_insert_with(|| Arc::new(WriterGate::new()));

        Ok(())
    }

    fn read(&self, namespace: &'static str) -> Result<Self::ReadHandle, Self::Error> {
        let snapshot = self.db.snapshot();
        let snapshot = unsafe {
            std::mem::transmute::<SnapshotWithThreadMode<'_, TransactionDB>, RocksSnapshot>(
                snapshot,
            )
        };

        Ok(RocksDbReadHandle {
            namespace,
            stores: self.stores.read().unwrap().clone(),
            inner: Arc::new(RocksDbReadInner {
                snapshot,
                _db: self.db.clone(),
            }),
        })
    }

    fn write(&self, namespace: &'static str) -> Result<Self::WriteHandle, Self::Error> {
        let gate = {
            let mut writers = self.writers.lock().unwrap();
            writers
                .entry(namespace)
                .or_insert_with(|| Arc::new(WriterGate::new()))
                .clone()
        };
        let permit = gate.acquire();

        let transaction = self
            .db
            .transaction_opt(&WriteOptions::default(), &transaction_options());
        let transaction = unsafe {
            std::mem::transmute::<Transaction<'_, TransactionDB>, RocksTransaction>(transaction)
        };

        Ok(RocksDbWriteHandle {
            namespace,
            stores: self.stores.read().unwrap().clone(),
            transaction,
            _permit: permit,
            _db: self.db.clone(),
        })
    }
}

#[doc(hidden)]
pub struct RocksDbReadHandle {
    namespace: &'static str,
    stores: Arc<PreparedStores>,
    inner: Arc<RocksDbReadInner>,
}

struct RocksDbReadInner {
    snapshot: RocksSnapshot,
    _db: Arc<TransactionDB>,
}

impl MultiStoreReadHandle for RocksDbReadHandle {
    type Error = BackendError;
    type Store = RocksDbReadStore;

    fn open_store(&self, name: &'static str) -> Result<Self::Store, Self::Error> {
        Ok(RocksDbReadStore {
            prefix: prepared_store_prefix(&self.stores, self.namespace, name),
            inner: self.inner.clone(),
        })
    }
}

#[doc(hidden)]
pub struct RocksDbWriteHandle {
    namespace: &'static str,
    stores: Arc<PreparedStores>,
    transaction: RocksTransaction,
    _permit: WriterPermit,
    _db: Arc<TransactionDB>,
}

impl MultiStoreWriteHandle for RocksDbWriteHandle {
    type Error = BackendError;
    type Store<'a> = RocksDbWriteStore<'a>;

    fn open_store(&mut self, name: &'static str) -> Result<Self::Store<'_>, Self::Error> {
        Ok(RocksDbWriteStore {
            prefix: prepared_store_prefix(&self.stores, self.namespace, name),
            transaction: &self.transaction,
        })
    }

    fn commit(self) -> Result<(), Self::Error> {
        self.transaction.commit().map_err(BackendError::new)
    }
}

#[doc(hidden)]
pub struct RocksDbReadStore {
    prefix: Arc<[u8]>,
    inner: Arc<RocksDbReadInner>,
}

impl ReadKVStore for RocksDbReadStore {
    type Error = BackendError;
    type Value<'a>
        = RocksDbValue
    where
        Self: 'a;
    type Entry = RocksDbEntry;
    type Iter = std::vec::IntoIter<ScanResult>;

    fn get(&self, key: impl AsRef<[u8]>) -> Result<Option<Self::Value<'_>>, Self::Error> {
        self.inner
            .snapshot
            .get(prefixed_key(&self.prefix, key.as_ref()))
            .map(|value| value.map(RocksDbValue))
            .map_err(BackendError::new)
    }

    fn scan(
        self,
        range: impl RangeBounds<Vec<u8>>,
        direction: Direction,
    ) -> Result<Self::Iter, Self::Error> {
        scan_entries(self.prefix.as_ref(), direction, range, |readopts, mode| {
            self.inner.snapshot.iterator_opt(mode, readopts)
        })
    }
}

#[doc(hidden)]
pub struct RocksDbWriteStore<'a> {
    prefix: Arc<[u8]>,
    transaction: &'a RocksTransaction,
}

impl<'a> ReadWriteKVStore<'a> for RocksDbWriteStore<'a> {
    type Error = BackendError;
}

impl<'a> WriteKVStore<'a> for RocksDbWriteStore<'a> {
    type Error = BackendError;

    fn set(&mut self, key: impl AsRef<[u8]>, value: impl AsRef<[u8]>) -> Result<(), Self::Error> {
        self.transaction
            .put(prefixed_key(&self.prefix, key.as_ref()), value.as_ref())
            .map_err(BackendError::new)
    }

    fn remove(&mut self, key: impl AsRef<[u8]>) -> Result<(), Self::Error> {
        self.transaction
            .delete(prefixed_key(&self.prefix, key.as_ref()))
            .map_err(BackendError::new)
    }
}

impl ReadKVStore for RocksDbWriteStore<'_> {
    type Error = BackendError;
    type Value<'a>
        = RocksDbValue
    where
        Self: 'a;
    type Entry = RocksDbEntry;
    type Iter = std::vec::IntoIter<ScanResult>;

    fn get(&self, key: impl AsRef<[u8]>) -> Result<Option<Self::Value<'_>>, Self::Error> {
        self.transaction
            .get(prefixed_key(&self.prefix, key.as_ref()))
            .map(|value| value.map(RocksDbValue))
            .map_err(BackendError::new)
    }

    fn scan(
        self,
        range: impl RangeBounds<Vec<u8>>,
        direction: Direction,
    ) -> Result<Self::Iter, Self::Error> {
        scan_entries(self.prefix.as_ref(), direction, range, |readopts, mode| {
            self.transaction.iterator_opt(mode, readopts)
        })
    }
}

#[doc(hidden)]
pub struct RocksDbValue(Vec<u8>);

impl AsRef<[u8]> for RocksDbValue {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

#[doc(hidden)]
pub struct RocksDbEntry {
    key: Vec<u8>,
    value: Vec<u8>,
}

impl KVEntry for RocksDbEntry {
    fn key(&self) -> &[u8] {
        self.key.as_slice()
    }

    fn value(&self) -> &[u8] {
        self.value.as_slice()
    }
}

struct WriterGate {
    open: Mutex<bool>,
    available: Condvar,
}

impl WriterGate {
    fn new() -> Self {
        Self {
            open: Mutex::new(false),
            available: Condvar::new(),
        }
    }

    fn acquire(self: &Arc<Self>) -> WriterPermit {
        let mut open = self.open.lock().unwrap();
        while *open {
            open = self.available.wait(open).unwrap();
        }
        *open = true;

        WriterPermit { gate: self.clone() }
    }
}

struct WriterPermit {
    gate: Arc<WriterGate>,
}

impl Drop for WriterPermit {
    fn drop(&mut self) {
        let mut open = self.gate.open.lock().unwrap();
        *open = false;
        self.gate.available.notify_one();
    }
}

fn default_options() -> Options {
    let mut opts = Options::default();
    opts.create_if_missing(true);
    opts
}

fn transaction_options() -> TransactionOptions {
    let mut opts = TransactionOptions::default();
    opts.set_snapshot(true);
    opts
}

fn prepared_store_prefix(
    stores: &PreparedStores,
    namespace: &'static str,
    store: &'static str,
) -> Arc<[u8]> {
    stores.get(&(namespace, store)).cloned().unwrap_or_else(|| {
        panic!(
            "store '{}' in namespace '{}' has not been prepared",
            store, namespace
        )
    })
}

fn store_prefix(namespace: &str, store: &str) -> Vec<u8> {
    let mut prefix = Vec::with_capacity(8 + namespace.len() + store.len());
    push_len_prefixed(&mut prefix, namespace.as_bytes());
    push_len_prefixed(&mut prefix, store.as_bytes());
    prefix
}

fn push_len_prefixed(out: &mut Vec<u8>, bytes: &[u8]) {
    let len = u32::try_from(bytes.len()).expect("namespace and store names fit in u32");
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(bytes);
}

fn prefixed_key(prefix: &[u8], key: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(prefix.len() + key.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(key);
    out
}

fn scan_entries<I>(
    prefix: &[u8],
    direction: Direction,
    range: impl RangeBounds<Vec<u8>>,
    make_iter: impl FnOnce(ReadOptions, IteratorMode<'_>) -> I,
) -> Result<std::vec::IntoIter<ScanResult>, BackendError>
where
    I: Iterator<Item = Result<(Box<[u8]>, Box<[u8]>), RocksError>>,
{
    let mut readopts = ReadOptions::default();
    readopts.set_iterate_lower_bound(prefix.to_vec());
    if let Some(upper) = prefix_end(prefix) {
        readopts.set_iterate_upper_bound(upper);
    }

    let mode = match direction {
        Direction::LeftToRight => IteratorMode::From(prefix, RocksDirection::Forward),
        Direction::RightToLeft => IteratorMode::End,
    };

    let entries = make_iter(readopts, mode)
        .filter_map(|entry| match entry {
            Ok((key, value)) => {
                let logical_key = key
                    .strip_prefix(prefix)
                    .expect("RocksDB iterator bounds only return keys with this prefix");

                range_contains(&range, logical_key).then(|| {
                    Ok(RocksDbEntry {
                        key: logical_key.to_vec(),
                        value: value.into_vec(),
                    })
                })
            }
            Err(error) => Some(Err(BackendError::new(error))),
        })
        .collect::<Vec<_>>();

    Ok(entries.into_iter())
}

fn range_contains(range: &impl RangeBounds<Vec<u8>>, key: &[u8]) -> bool {
    let start_matches = match range.start_bound() {
        Bound::Included(start) => key >= start.as_slice(),
        Bound::Excluded(start) => key > start.as_slice(),
        Bound::Unbounded => true,
    };
    let end_matches = match range.end_bound() {
        Bound::Included(end) => key <= end.as_slice(),
        Bound::Excluded(end) => key < end.as_slice(),
        Bound::Unbounded => true,
    };

    start_matches && end_matches
}

fn prefix_end(prefix: &[u8]) -> Option<Vec<u8>> {
    let mut end = prefix.to_vec();
    for byte in end.iter_mut().rev() {
        if *byte != u8::MAX {
            *byte += 1;
            end.truncate(end.len());
            return Some(end);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::tests::multistore_contract_tests;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DB: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn prefixes_encode_namespace_and_store_boundaries() {
        assert_ne!(store_prefix("ab", "c"), store_prefix("a", "bc"));
    }

    #[test]
    #[should_panic(expected = "store 'missing' in namespace 'users' has not been prepared")]
    fn write_open_store_panics_for_unprepared_stores() {
        let db = make_db();
        db.prepare("users", ["__main"]).unwrap();

        let mut write = db.write("users").unwrap();

        write.open_store("missing").unwrap();
    }

    #[test]
    fn prepare_replaces_namespace_registry_snapshot() {
        let db = make_db();
        db.prepare("users", ["old"]).unwrap();

        let read_before = db.read("users").unwrap();

        db.prepare("users", ["new"]).unwrap();

        assert!(read_before.open_store("old").is_ok());
        assert_panics(|| {
            read_before.open_store("new").unwrap();
        });

        let read_after = db.read("users").unwrap();
        assert_panics(|| {
            read_after.open_store("old").unwrap();
        });
        assert!(read_after.open_store("new").is_ok());
    }

    fn assert_panics(f: impl FnOnce()) {
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).is_err());
    }

    fn make_db() -> RocksDbMultiStore {
        RocksDbMultiStore::create(temp_db_path()).unwrap()
    }

    fn temp_db_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "collette-rocksdb-contract-{}-{}",
            std::process::id(),
            NEXT_DB.fetch_add(1, Ordering::Relaxed)
        ))
    }

    multistore_contract_tests!(make_db);
}
