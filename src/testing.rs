//! Reusable mock implementations of the [`crate::store`] traits for use across
//! test modules.
//!
//! Every operation performed through a [`MockDb`] is recorded in a shared
//! [`TxLog`] that callers can inspect after the fact.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::{Bound, RangeBounds};
use std::rc::Rc;

use crate::error::{BackendError, Error};
use crate::index_registry::IndexRegistry;
use crate::item::Item;
use crate::scan::Direction;
use crate::store::{
    MultiStore, MultiStoreReadHandle, MultiStoreWriteHandle, ReadKVStore, ReadWriteKVStore,
    WriteKVStore,
};

// ── Error helpers ─────────────────────────────────────────────────────────────

/// Constructs a [`BackendError`] from a static message, for use in error
/// factory functions passed to [`MockDb::with_write_err`] /
/// [`MockDb::with_commit_err`].
pub fn backend_error(msg: &'static str) -> BackendError {
    BackendError::new(std::io::Error::new(std::io::ErrorKind::Other, msg))
}

// ── TxLog ─────────────────────────────────────────────────────────────────────

/// Shared log of all store operations performed via a [`MockDb`] instance.
///
/// The log is shared between the db and all handles/stores it produces so
/// that callers can observe the full sequence of operations after a test call.
#[derive(Default, Debug)]
pub struct TxLog {
    /// Names of stores opened via `open_store` (in call order).
    pub opens: Vec<String>,
    /// Raw keys passed to `get` (in call order).
    pub gets: Vec<Vec<u8>>,
    /// `(key, value)` pairs passed to `set` (in call order).
    pub sets: Vec<(Vec<u8>, Vec<u8>)>,
    /// Raw keys passed to `remove` (in call order).
    pub removes: Vec<Vec<u8>>,
    /// Ranges passed to `scan` (in call order).
    pub scans: Vec<ScanLog>,
    /// Whether `commit` was called on the write handle.
    pub committed: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ScanLog {
    pub left: Bound<Vec<u8>>,
    pub right: Bound<Vec<u8>>,
    pub direction: Direction,
}

// ── MockStore ─────────────────────────────────────────────────────────────────

/// A mock [`ReadWriteKVStore`] that records every operation in a shared
/// [`TxLog`] and serves pre-configured byte values for `get` calls.
pub struct MockStore {
    log: Rc<RefCell<TxLog>>,
    data: HashMap<Vec<u8>, Vec<u8>>,
}

impl ReadKVStore for MockStore {
    type Error = BackendError;
    type Iter = std::iter::Empty<Result<(Vec<u8>, Vec<u8>), BackendError>>;

    fn get(&self, key: impl AsRef<[u8]>) -> Result<Option<Vec<u8>>, BackendError> {
        let key = key.as_ref().to_vec();
        self.log.borrow_mut().gets.push(key.clone());
        Ok(self.data.get(&key).cloned())
    }

    fn scan(
        self,
        range: impl RangeBounds<Vec<u8>>,
        direction: Direction,
    ) -> Result<Self::Iter, BackendError> {
        self.log.borrow_mut().scans.push(ScanLog {
            left: range.start_bound().cloned(),
            right: range.end_bound().cloned(),
            direction,
        });
        Ok(std::iter::empty())
    }
}

impl<'a> WriteKVStore<'a> for MockStore {
    type Error = BackendError;

    fn set(&mut self, key: impl AsRef<[u8]>, value: impl AsRef<[u8]>) -> Result<(), BackendError> {
        self.log
            .borrow_mut()
            .sets
            .push((key.as_ref().to_vec(), value.as_ref().to_vec()));
        Ok(())
    }

    fn remove(&mut self, key: impl AsRef<[u8]>) -> Result<(), BackendError> {
        self.log.borrow_mut().removes.push(key.as_ref().to_vec());
        Ok(())
    }
}

impl<'a> ReadWriteKVStore<'a> for MockStore {
    type Error = BackendError;
}

// ── MockWriteHandle ───────────────────────────────────────────────────────────

/// A mock [`MultiStoreWriteHandle`].
///
/// Each call to `open_store` is logged and returns a [`MockStore`] seeded with
/// the data registered under that store name on the owning [`MockDb`].
pub struct MockWriteHandle {
    log: Rc<RefCell<TxLog>>,
    store_data: HashMap<String, HashMap<Vec<u8>, Vec<u8>>>,
    commit_err: Option<fn() -> BackendError>,
}

impl MultiStoreWriteHandle for MockWriteHandle {
    type Error = BackendError;
    type Store<'a> = MockStore;

    fn open_store(&mut self, name: &'static str) -> Result<MockStore, BackendError> {
        self.log.borrow_mut().opens.push(name.to_string());
        let data = self.store_data.get(name).cloned().unwrap_or_default();
        Ok(MockStore {
            log: self.log.clone(),
            data,
        })
    }

    fn commit(self) -> Result<(), BackendError> {
        self.log.borrow_mut().committed = true;
        match self.commit_err {
            Some(make_err) => Err(make_err()),
            None => Ok(()),
        }
    }
}

// ── MockReadHandle ────────────────────────────────────────────────────────────

/// A mock [`MultiStoreReadHandle`].
///
/// Read operations are also recorded in the shared log.
pub struct MockReadHandle {
    log: Rc<RefCell<TxLog>>,
    store_data: HashMap<String, HashMap<Vec<u8>, Vec<u8>>>,
}

impl MultiStoreReadHandle for MockReadHandle {
    type Error = BackendError;
    type Store = MockStore;

    fn open_store(&self, name: &'static str) -> Result<MockStore, BackendError> {
        self.log.borrow_mut().opens.push(name.to_string());
        let data = self.store_data.get(name).cloned().unwrap_or_default();
        Ok(MockStore {
            log: self.log.clone(),
            data,
        })
    }
}

// ── MockDb ────────────────────────────────────────────────────────────────────

/// A configurable mock [`MultiStore`].
///
/// # Usage
///
/// ```rust,ignore
/// let db = MockDb::new()
///     .with_data("__main", enc_pk, enc_val)   // simulate existing record
///     .with_commit_err(|| backend_error("disk full"));
///
/// let log = db.log();  // clone the Rc before the db is moved
/// collection.insert(record)?;
///
/// let log = log.borrow();
/// assert_eq!(log.sets.len(), 1);
/// assert!(log.committed);
/// ```
pub struct MockDb {
    log: Rc<RefCell<TxLog>>,
    store_data: HashMap<String, HashMap<Vec<u8>, Vec<u8>>>,
    read_err: Option<fn() -> BackendError>,
    write_err: Option<fn() -> BackendError>,
    commit_err: Option<fn() -> BackendError>,
}

impl Default for MockDb {
    fn default() -> Self {
        Self {
            log: Rc::new(RefCell::new(TxLog::default())),
            store_data: HashMap::new(),
            read_err: None,
            write_err: None,
            commit_err: None,
        }
    }
}

impl MockDb {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a handle to the shared operation log for post-call assertions.
    ///
    /// Clone this before moving `MockDb` into a [`Collection`].
    pub fn log(&self) -> Rc<RefCell<TxLog>> {
        self.log.clone()
    }

    /// Pre-seeds a named store with a key/value entry (returned by `get`).
    pub fn with_data(
        mut self,
        store: &str,
        key: impl Into<Vec<u8>>,
        value: impl Into<Vec<u8>>,
    ) -> Self {
        self.store_data
            .entry(store.to_string())
            .or_default()
            .insert(key.into(), value.into());
        self
    }

    /// Makes `read()` return an error produced by `make_err`.
    pub fn with_read_err(mut self, make_err: fn() -> BackendError) -> Self {
        self.read_err = Some(make_err);
        self
    }

    /// Makes `write()` return an error produced by `make_err`.
    pub fn with_write_err(mut self, make_err: fn() -> BackendError) -> Self {
        self.write_err = Some(make_err);
        self
    }

    /// Makes `commit()` return an error produced by `make_err`.
    pub fn with_commit_err(mut self, make_err: fn() -> BackendError) -> Self {
        self.commit_err = Some(make_err);
        self
    }
}

impl MultiStore for MockDb {
    type Error = BackendError;
    type ReadHandle = MockReadHandle;
    type WriteHandle = MockWriteHandle;

    fn prepare(
        &self,
        _namespace: &'static str,
        _stores: impl IntoIterator<Item = &'static str>,
    ) -> Result<(), BackendError> {
        Ok(())
    }

    fn read(&self, _: &'static str) -> Result<MockReadHandle, BackendError> {
        if let Some(make_err) = self.read_err {
            return Err(make_err());
        }
        Ok(MockReadHandle {
            log: self.log.clone(),
            store_data: self.store_data.clone(),
        })
    }

    fn write(&self, _: &'static str) -> Result<MockWriteHandle, BackendError> {
        if let Some(make_err) = self.write_err {
            return Err(make_err());
        }
        Ok(MockWriteHandle {
            log: self.log.clone(),
            store_data: self.store_data.clone(),
            commit_err: self.commit_err,
        })
    }
}

// ── SpyRegistry ───────────────────────────────────────────────────────────────

thread_local! {
    static REGISTRY_UPDATE_CALLED: Cell<bool> = Cell::new(false);
    static REGISTRY_REMOVE_CALLED: Cell<bool> = Cell::new(false);
    static REGISTRY_SHOULD_FAIL: Cell<bool>   = Cell::new(false);
}

/// A mock [`IndexRegistry`] backed by thread-local flags.
///
/// Each test thread starts with a clean slate. Call [`SpyRegistry::reset`]
/// between successive uses within the same test function.
pub struct SpyRegistry;

impl SpyRegistry {
    /// Resets all flags to their initial state.
    pub fn reset() {
        REGISTRY_UPDATE_CALLED.with(|c| c.set(false));
        REGISTRY_REMOVE_CALLED.with(|c| c.set(false));
        REGISTRY_SHOULD_FAIL.with(|c| c.set(false));
    }

    /// When set to `true`, the next `update` or `remove` call returns
    /// `Err(Error::Unexpected(...))`.
    pub fn set_fail(fail: bool) {
        REGISTRY_SHOULD_FAIL.with(|c| c.set(fail));
    }

    /// Returns `true` if `update` was called since the last [`reset`].
    pub fn was_update_called() -> bool {
        REGISTRY_UPDATE_CALLED.with(|c| c.get())
    }

    /// Returns `true` if `remove` was called since the last [`reset`].
    pub fn was_remove_called() -> bool {
        REGISTRY_REMOVE_CALLED.with(|c| c.get())
    }

    fn fail_if_needed() -> Result<(), Error> {
        if REGISTRY_SHOULD_FAIL.with(|c| c.get()) {
            Err(Error::Unexpected("injected registry error".into()))
        } else {
            Ok(())
        }
    }
}

impl<T: Item> IndexRegistry<T> for SpyRegistry {
    fn store_names(_out: &mut Vec<&'static str>) {}

    fn update<'a, DB: MultiStoreWriteHandle>(
        _db: &mut DB,
        _pk: &T::Key<'a>,
        _old: Option<&T>,
        _new: &'a T,
    ) -> Result<(), Error> {
        REGISTRY_UPDATE_CALLED.with(|c| c.set(true));
        Self::fail_if_needed()
    }

    fn remove<'a, DB: MultiStoreWriteHandle>(
        _db: &mut DB,
        _pk: &T::Key<'a>,
        _item: &'a T,
    ) -> Result<(), Error> {
        REGISTRY_REMOVE_CALLED.with(|c| c.set(true));
        Self::fail_if_needed()
    }

    fn has_index(_name: &str) -> bool {
        false
    }
}
