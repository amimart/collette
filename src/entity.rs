use crate::error::CodecError;
use crate::key::Key;

/// A persistable record stored inside a Collette collection.
///
/// An entity defines:
///
/// - its primary key type;
/// - how its primary key is accessed;
/// - how it is encoded and decoded from storage bytes.
pub trait Entity: Sized {
    /// The primary key type of the entity.
    ///
    /// The key may borrow from `self` to avoid allocations.
    type Key<'a>: Key
    where
        Self: 'a;

    /// Returns the primary key of the entity.
    ///
    /// Implementations should prefer returning borrowed keys when possible.
    fn key(&self) -> Self::Key<'_>;

    /// Encodes the entity into storage bytes.
    ///
    /// The encoded representation is stored as the collection value inside the
    /// underlying KV store.
    fn to_bytes(&self) -> Result<Vec<u8>, CodecError>;

    /// Decodes an entity from storage bytes.
    ///
    /// Implementations should return an error if the input bytes are malformed
    /// or incompatible with the expected entity format.
    fn from_bytes(bytes: &[u8]) -> Result<Self, CodecError>;
}
