//! Error types used by Collette.

use std::fmt::Display;

/// Error returned by collection-level operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Insert or unique-index update attempted to create an existing key.
    #[error("key already exists: {0}")]
    AlreadyExists(String),

    /// Update attempted to modify a missing record.
    #[error("key not found: {0}")]
    NotFound(String),

    /// A scan cursor was outside the configured scan bounds.
    #[error("cursor is outside scan bounds")]
    CursorOutOfBounds,

    /// Error reported by the storage backend.
    #[error("backend error: {0}")]
    Backend(#[from] BackendError),

    /// Error reported by an entity codec.
    #[error("serialization error: {0}")]
    Codec(#[from] CodecError),

    /// Internal invariant failure.
    #[error("unexpected error: {0}")]
    Unexpected(String),
}

/// Type-erased error returned by a [`MultiStore`](crate::store::MultiStore) backend.
#[derive(Debug, thiserror::Error)]
pub struct BackendError(Box<dyn std::error::Error + Send + Sync>);

impl BackendError {
    #[cfg(any(test, feature = "redb"))]
    pub(crate) fn new(e: impl std::error::Error + Send + Sync + 'static) -> Self {
        BackendError(Box::new(e))
    }
}

impl Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Type-erased error returned by [`Entity`](crate::Entity) serialization.
#[derive(Debug, thiserror::Error)]
pub struct CodecError(Box<dyn std::error::Error + Send + Sync>);

impl CodecError {
    #[cfg(test)]
    pub(crate) fn new(e: impl std::error::Error + Send + Sync + 'static) -> Self {
        CodecError(Box::new(e))
    }
}

impl Display for CodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
