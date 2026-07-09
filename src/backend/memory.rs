//! In-memory [`MultiStore`] backend.
//!
//! This backend is useful for tests, examples, and ephemeral in-process state.
//! Reads see stable snapshots, while writes are staged and published atomically
//! on commit.

use crate::error::BackendError;
use crate::scan::Direction;
use crate::store::{
    MultiStore, MultiStoreReadHandle, MultiStoreWriteHandle, ReadKVStore, ReadWriteKVStore,
    WriteKVStore,
};
use std::collections::BTreeMap;
use std::ops::RangeBounds;
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::vec::IntoIter;

/// In-process ordered key-value backend.
///
/// Data is kept in memory and lost when the value is dropped.
pub struct InMemoryMultiStore {
    stores: SharedNamespaces,
}

#[doc(hidden)]
pub type SharedNamespaces = Arc<RwLock<Namespaces>>;
#[doc(hidden)]
pub type Namespaces = BTreeMap<&'static str, Arc<NamespacedState>>;
#[doc(hidden)]
pub type NamespacedStores = BTreeMap<&'static str, Arc<KVStore>>;
#[doc(hidden)]
pub type StagedStores = BTreeMap<&'static str, KVStore>;
#[doc(hidden)]
pub type KVStore = BTreeMap<Vec<u8>, Vec<u8>>;
#[doc(hidden)]
pub type ScanItem = (Vec<u8>, Vec<u8>);
#[doc(hidden)]
pub type ScanResult = Result<ScanItem, BackendError>;
#[doc(hidden)]
pub type ScanResults = Vec<ScanResult>;

#[doc(hidden)]
pub struct NamespacedState {
    stores: RwLock<Arc<NamespacedStores>>,
    writer: Arc<WriterGate>,
}

impl NamespacedState {
    fn new(stores: NamespacedStores) -> Self {
        Self {
            stores: RwLock::new(Arc::new(stores)),
            writer: Arc::new(WriterGate::new()),
        }
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

impl Default for InMemoryMultiStore {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryMultiStore {
    /// Creates an empty in-memory backend.
    pub fn new() -> Self {
        Self {
            stores: Arc::from(RwLock::from(BTreeMap::new())),
        }
    }
}

impl MultiStore for InMemoryMultiStore {
    type ReadHandle = InMemoryReadHandle;
    type WriteHandle = InMemoryWriteHandle;

    fn prepare(
        &self,
        namespace: &'static str,
        stores: impl IntoIterator<Item = &'static str>,
    ) -> Result<(), BackendError> {
        let stores = stores.into_iter().collect::<Vec<_>>();
        let mut db = self.stores.write().unwrap();

        if let Some(state) = db.get(namespace) {
            let current = state.stores.read().unwrap().clone();
            let next = stores
                .into_iter()
                .map(|store| {
                    (
                        store,
                        current
                            .get(store)
                            .cloned()
                            .unwrap_or_else(|| Arc::new(KVStore::new())),
                    )
                })
                .collect();
            *state.stores.write().unwrap() = Arc::new(next);
        } else {
            let nstores = stores
                .into_iter()
                .map(|store| (store, Arc::new(KVStore::new())))
                .collect();
            db.insert(namespace, Arc::new(NamespacedState::new(nstores)));
        }

        Ok(())
    }

    fn read(&self, namespace: &'static str) -> Result<Self::ReadHandle, BackendError> {
        let db = self.stores.read().unwrap();
        let state = db.get(namespace).unwrap();
        let snapshot = state.stores.read().unwrap().clone();

        Ok(InMemoryReadHandle { stores: snapshot })
    }

    fn write(&self, namespace: &'static str) -> Result<Self::WriteHandle, BackendError> {
        let db = self.stores.read().unwrap();
        let state = db.get(namespace).unwrap();
        let permit = state.writer.acquire();
        let snapshot = state.stores.read().unwrap().clone();
        let staged = snapshot
            .iter()
            .map(|(n, s)| (*n, s.as_ref().clone()))
            .collect();

        Ok(InMemoryWriteHandle {
            namespace: state.clone(),
            staged,
            _permit: permit,
        })
    }
}

#[doc(hidden)]
pub struct InMemoryReadHandle {
    stores: Arc<NamespacedStores>,
}

impl MultiStoreReadHandle for InMemoryReadHandle {
    type Store = InMemoryReadStore;

    fn open_store(&self, name: &'static str) -> Result<Self::Store, BackendError> {
        Ok(InMemoryReadStore {
            store: self.stores.get(name).unwrap().clone(),
        })
    }
}

#[doc(hidden)]
pub struct InMemoryReadStore {
    store: Arc<KVStore>,
}

impl ReadKVStore for InMemoryReadStore {
    type Iter = IntoIter<ScanResult>;

    fn get(&self, key: impl AsRef<[u8]>) -> Result<Option<Vec<u8>>, BackendError> {
        Ok(self.store.get(key.as_ref()).cloned())
    }

    fn scan(
        self,
        range: impl RangeBounds<Vec<u8>>,
        direction: Direction,
    ) -> Result<Self::Iter, BackendError> {
        let scan: ScanResults = match direction {
            Direction::LeftToRight => self
                .store
                .range(range)
                .map(|(k, v)| Ok((k.clone(), v.clone())))
                .collect(),
            Direction::RightToLeft => self
                .store
                .range(range)
                .rev()
                .map(|(k, v)| Ok((k.clone(), v.clone())))
                .collect(),
        };

        Ok(scan.into_iter())
    }
}

#[doc(hidden)]
pub struct InMemoryWriteHandle {
    namespace: Arc<NamespacedState>,
    staged: StagedStores,
    _permit: WriterPermit,
}

impl MultiStoreWriteHandle for InMemoryWriteHandle {
    type Store<'a> = InMemoryWriteStore<'a>;

    fn open_store(&mut self, name: &'static str) -> Result<Self::Store<'_>, BackendError> {
        Ok(InMemoryWriteStore {
            store: self.staged.get_mut(name).unwrap(),
        })
    }

    fn commit(self) -> Result<(), BackendError> {
        let new_stores = self
            .staged
            .into_iter()
            .map(|(n, s)| (n, Arc::new(s)))
            .collect();

        let mut stores = self.namespace.stores.write().unwrap();
        *stores = Arc::new(new_stores);
        Ok(())
    }
}

#[doc(hidden)]
pub struct InMemoryWriteStore<'a> {
    store: &'a mut KVStore,
}

impl<'a> ReadWriteKVStore<'a> for InMemoryWriteStore<'a> {}

impl<'a> WriteKVStore<'a> for InMemoryWriteStore<'a> {
    fn set(&mut self, key: impl AsRef<[u8]>, value: impl AsRef<[u8]>) -> Result<(), BackendError> {
        self.store
            .insert(key.as_ref().to_owned(), value.as_ref().to_owned());
        Ok(())
    }

    fn remove(&mut self, key: impl AsRef<[u8]>) -> Result<(), BackendError> {
        self.store.remove(&key.as_ref().to_owned());
        Ok(())
    }
}

impl ReadKVStore for InMemoryWriteStore<'_> {
    type Iter = IntoIter<ScanResult>;

    fn get(&self, key: impl AsRef<[u8]>) -> Result<Option<Vec<u8>>, BackendError> {
        Ok(self.store.get(key.as_ref()).map(|v| v.to_owned()))
    }

    fn scan(
        self,
        range: impl RangeBounds<Vec<u8>>,
        direction: Direction,
    ) -> Result<Self::Iter, BackendError> {
        let scan: ScanResults = match direction {
            Direction::LeftToRight => self
                .store
                .range(range)
                .map(|(k, v)| Ok((k.clone(), v.clone())))
                .collect(),
            Direction::RightToLeft => self
                .store
                .range(range)
                .rev()
                .map(|(k, v)| Ok((k.clone(), v.clone())))
                .collect(),
        };

        Ok(scan.into_iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::tests::multistore_contract_tests;

    fn make_db() -> InMemoryMultiStore {
        InMemoryMultiStore::new()
    }

    multistore_contract_tests!(make_db);
}
