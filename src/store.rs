//! Storage adapter traits implemented by multistore backends.
//!
//! A [`MultiStore`](crate::store::MultiStore) exposes named namespaces, each
//! containing several ordered key-value stores. Collections use one primary
//! store plus one store per secondary index.
//!
//! These traits are backend integration points, not the normal application API.
//! End users should create a [`Collection`](crate::Collection) with a [`MultiStore`](crate::store::MultiStore)
//! implementation and then call collection methods. This API shall never be used directly.

use crate::scan::Direction;
use std::ops::RangeBounds;

/// Adapter contract for a backend that can expose several ordered KV stores.
///
/// Implement this trait to make Collette work with a new key-value store. In
/// application code, prefer the collection API; `prepare`, `read`, and `write`
/// are called by Collette.
pub trait MultiStore {
    /// Error returned by this backend.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Read-only transaction or snapshot handle.
    type ReadHandle: MultiStoreReadHandle<Error = Self::Error>;
    /// Write transaction handle.
    type WriteHandle: MultiStoreWriteHandle<Error = Self::Error>;

    /// Initializes the given stores for a namespace.
    ///
    /// This is called automatically when a collection is built. Backend
    /// implementations should make it safe to call repeatedly for the same
    /// namespace and stores.
    fn prepare(
        &self,
        namespace: &'static str,
        stores: impl IntoIterator<Item = &'static str>,
    ) -> Result<(), Self::Error>;

    /// Opens a read-only handle for the namespace.
    ///
    /// Collette calls this while serving collection reads and scans.
    fn read(&self, namespace: &'static str) -> Result<Self::ReadHandle, Self::Error>;

    /// Opens a write handle for the namespace.
    ///
    /// Collette calls this for collection mutations. All writes to stores
    /// opened from this handle become visible atomically when
    /// [`commit`](MultiStoreWriteHandle::commit) succeeds.
    fn write(&self, namespace: &'static str) -> Result<Self::WriteHandle, Self::Error>;
}

/// Read-only view of the stores in one namespace.
pub trait MultiStoreReadHandle {
    /// Error returned by this read handle.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Store type returned by this read handle.
    type Store: ReadKVStore<Error = Self::Error>;

    /// Opens one prepared store by name.
    fn open_store(&self, name: &'static str) -> Result<Self::Store, Self::Error>;
}

/// Write transaction across all stores opened from one namespace.
pub trait MultiStoreWriteHandle {
    /// Error returned by this write handle.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Mutable store type returned by this write handle.
    type Store<'a>: ReadWriteKVStore<'a, Error = Self::Error>
    where
        Self: 'a;

    /// Opens one prepared store by name.
    fn open_store(&mut self, name: &'static str) -> Result<Self::Store<'_>, Self::Error>;

    /// Commits all changes made through this handle.
    fn commit(self) -> Result<(), Self::Error>;
}

/// Store that can be read from and written to in the same transaction.
pub trait ReadWriteKVStore<'a>:
    ReadKVStore<Error = <Self as ReadWriteKVStore<'a>>::Error>
    + WriteKVStore<'a, Error = <Self as ReadWriteKVStore<'a>>::Error>
{
    /// Error returned by this store.
    type Error: std::error::Error + Send + Sync + 'static;
}

/// Mutable ordered key-value store.
pub trait WriteKVStore<'a> {
    /// Error returned by this store.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Sets or replaces a key-value pair.
    fn set(&mut self, key: impl AsRef<[u8]>, value: impl AsRef<[u8]>) -> Result<(), Self::Error>;

    /// Removes a key-value pair if it exists.
    fn remove(&mut self, key: impl AsRef<[u8]>) -> Result<(), Self::Error>;
}

/// Key-value bytes yielded by a backend scan.
///
/// Implementations may own bytes, borrow bytes, or hold backend guard objects
/// that keep the returned byte slices valid.
pub trait KVEntry {
    /// Encoded key bytes.
    fn key(&self) -> &[u8];

    /// Encoded value bytes.
    fn value(&self) -> &[u8];
}

/// Read-only ordered key-value store.
pub trait ReadKVStore {
    /// Error returned by this store.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Value returned by [`get`](Self::get).
    ///
    /// This can be a borrowed slice, an owned byte buffer, or a backend guard
    /// object exposing bytes through [`AsRef<[u8]>`](AsRef).
    type Value<'a>: AsRef<[u8]>
    where
        Self: 'a;

    /// Entry yielded by [`scan`](Self::scan).
    ///
    /// This keeps scan results from forcing key/value copies at the backend
    /// trait boundary.
    type Entry: KVEntry;

    /// Iterator returned by [`scan`](Self::scan).
    type Iter: Iterator<Item = Result<Self::Entry, Self::Error>>;

    /// Gets a value by exact key.
    fn get(&self, key: impl AsRef<[u8]>) -> Result<Option<Self::Value<'_>>, Self::Error>;

    /// Scans key-value pairs in byte-key order inside `range`.
    fn scan(
        self,
        range: impl RangeBounds<Vec<u8>>,
        direction: Direction,
    ) -> Result<Self::Iter, Self::Error>;
}
