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
    Backend(#[source] BackendError),

    /// Error reported by an item codec.
    #[error("serialization error: {0}")]
    Codec(#[source] CodecError),

    /// Internal invariant failure.
    #[error("unexpected error: {0}")]
    Unexpected(String),
}

impl Error {
    pub(crate) fn backend(e: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Backend(BackendError::new(e))
    }

    pub(crate) fn codec(e: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Codec(CodecError::new(e))
    }
}

/// Type-erased error returned by a [`MultiStore`](crate::store::MultiStore) backend.
#[derive(Debug)]
pub struct BackendError(BoxError);

impl BackendError {
    pub(crate) fn new(e: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self(Box::new(e))
    }
}

impl Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for BackendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}

/// Type-erased error returned by an [`Item`](crate::Item) codec.
#[derive(Debug)]
pub struct CodecError(BoxError);

impl CodecError {
    pub(crate) fn new(e: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self(Box::new(e))
    }
}

impl Display for CodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for CodecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
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
        assert_eq!(
            err.source().unwrap().source().unwrap().to_string(),
            "bad bytes"
        );
    }

    #[test]
    fn backend_error_preserves_user_error_as_source() {
        let err = Error::backend(std::io::Error::other("disk said no"));

        assert_eq!(err.to_string(), "backend error: disk said no");
        assert_eq!(err.source().unwrap().to_string(), "disk said no");
        assert_eq!(
            err.source().unwrap().source().unwrap().to_string(),
            "disk said no"
        );
    }
}
