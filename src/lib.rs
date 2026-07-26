//! Typed collections, indexes, and scans over ordered key-value stores.
//!
//! Collette sits between raw embedded KV APIs and heavier ORM/database layers:
//! you define an [`Item`], describe the secondary [`Index`] values that should
//! be maintained for it, and query those indexes with typed range and prefix
//! scans.
//!
//! The crate is built around a few small contracts:
//!
//! - [`Item`] defines record serialization and primary-key extraction.
//! - [`Key`] defines the ordered byte encoding used by primary keys, index keys,
//!   scan bounds, and cursors.
//! - [`Index`] describes a secondary lookup over an item.
//! - [`collection()`] builds a [`Collection`] over a multistore backend.
//!
//! Backend traits such as [`store::MultiStore`] are adapter contracts for
//! storage backends. Application code should not call them directly; collection
//! construction and collection methods call into the backend for you.
//!
//! # Quick Start
//!
//! ```no_run
//! use collette::backend::memory::InMemoryMultiStore;
//! use collette::collection::collection;
//! use collette::item::Item;
//! use collette::index::{Index, Unique};
//!
//! #[derive(Clone)]
//! struct User {
//!     id: u64,
//!     email: String,
//! }
//!
//! impl Item for User {
//!     type Key<'a> = u64;
//!     type Error = std::convert::Infallible;
//!
//!     fn key(&self) -> Self::Key<'_> {
//!         self.id
//!     }
//!
//!     fn to_bytes(&self) -> Result<Vec<u8>, Self::Error> {
//!         Ok(self.email.as_bytes().to_vec())
//!     }
//!
//!     fn from_bytes(bytes: &[u8]) -> Result<Self, Self::Error> {
//!         Ok(Self {
//!             id: 0,
//!             email: String::from_utf8_lossy(bytes).into_owned(),
//!         })
//!     }
//! }
//!
//! struct ByEmail;
//!
//! impl Index<User> for ByEmail {
//!     type Key<'a> = &'a str;
//!     type Kind<'a> = Unique;
//!
//!     const NAME: &'static str = "by_email";
//!
//!     fn key(user: &User) -> Self::Key<'_> {
//!         user.email.as_str()
//!     }
//! }
//!
//! let db = InMemoryMultiStore::new();
//!
//! let users = collection::<User, _>("users", db)
//!     .with_index::<ByEmail>()
//!     .build();
//!
//! users.insert(User {
//!     id: 1,
//!     email: "ada@example.com".to_owned(),
//! })?;
//!
//! let ada = users.get(1)?;
//! # Ok::<(), collette::error::Error>(())
//! ```
//!
//! # Backends
//!
//! The default `memory` feature provides
//! [`InMemoryMultiStore`](backend::memory::InMemoryMultiStore), which is useful
//! for tests and in-process state. Enable the `redb` feature for the persistent
//! [`RedbMultiStore`](backend::redb::RedbMultiStore) backend.
//!
//! # Storage compatibility
//!
//! Collette stores exactly the bytes produced by your [`Item`] and [`Key`]
//! implementations. Changing either encoding changes the physical storage
//! layout and should be handled as an application migration.

#![cfg_attr(docsrs, feature(doc_cfg))]

pub use collection::{collection, Collection, CollectionBuilder};
pub use error::{BackendError, CodecError, Error};
pub use index::{Index, Multi, Unique};
pub use item::Item;
pub use iter::Cursor;
pub use key::{Key, KeySize};
pub use scan::{
    AfterScan, CollectionScan, CompiledScan, DirectedScan, Direction, IndexScan, PrefixableScan,
    PrefixedScan, RangeScan, Scan, ScanExecutor,
};

/// Backend implementations for Collette's storage traits.
pub mod backend;
/// Typed collection builder and record operations.
pub mod collection;
/// Error types returned by collection, codec, and backend operations.
pub mod error;
/// Secondary index traits and index cardinality markers.
pub mod index;
#[doc(hidden)]
pub mod index_registry;
mod inline_vec;
/// Collection item contract.
pub mod item;
/// Iterator types returned by collection and index scans.
pub mod iter;
/// Ordered key encoding traits and helpers.
pub mod key;
/// Macros for implementing ordered key encodings.
pub mod macros;
/// Prefix-bound helpers for scanning composite keys.
pub mod prefix;
/// Typed collection and index scan builders.
pub mod scan;
/// Backend storage adapter traits.
pub mod store;

#[doc(hidden)]
pub mod bounds;
#[cfg(test)]
pub mod testing;
