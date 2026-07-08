use std::fmt::Display;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("key already exists: {0}")]
    AlreadyExists(String),

    #[error("key not found: {0}")]
    NotFound(String),

    #[error("cursor is outside scan bounds")]
    CursorOutOfBounds,

    #[error("backend error: {0}")]
    Backend(#[from] BackendError),

    #[error("serialization error: {0}")]
    Codec(#[from] CodecError),

    #[error("unexpected error: {0}")]
    Unexpected(String),
}

#[derive(Debug, thiserror::Error)]
pub struct BackendError(Box<dyn std::error::Error + Send + Sync>);

impl BackendError {
    pub(crate) fn new(e: impl std::error::Error + Send + Sync + 'static) -> Self {
        BackendError(Box::new(e))
    }
}

impl Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

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
