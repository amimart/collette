//! Error types used by Collette.

use std::fmt::Display;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

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
    Codec(#[source] BoxError),

    /// Internal invariant failure.
    #[error("unexpected error: {0}")]
    Unexpected(String),
}

impl Error {
    pub(crate) fn codec(e: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Codec(Box::new(e))
    }
}

/// Type-erased error returned by a [`MultiStore`](crate::store::MultiStore) backend.
#[derive(Debug, thiserror::Error)]
pub struct BackendError(BoxError);

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

#[cfg(test)]
mod tests {
    use super::Error;
    use std::error::Error as _;

    #[test]
    fn codec_error_preserves_user_error_as_source() {
        let err = Error::codec(std::io::Error::other("bad bytes"));

        assert_eq!(err.to_string(), "serialization error: bad bytes");
        assert_eq!(err.source().unwrap().to_string(), "bad bytes");
    }
}
