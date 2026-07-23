//! Storage backends bundled with Collette.

#[cfg_attr(docsrs, doc(cfg(feature = "memory")))]
#[cfg(feature = "memory")]
pub mod memory;

#[cfg_attr(docsrs, doc(cfg(feature = "redb")))]
#[cfg(feature = "redb")]
pub mod redb;

#[cfg_attr(docsrs, doc(cfg(feature = "rocksdb")))]
#[cfg(feature = "rocksdb")]
pub mod rocksdb;

#[cfg(test)]
mod tests;
