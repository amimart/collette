use crate::key::Key;

/// A persistable record stored inside a Collette collection.
///
/// An entity defines:
///
/// - its primary key type;
/// - how its primary key is accessed;
/// - how it is encoded and decoded from storage bytes.
///
/// # Examples
///
/// ```
/// use collette::Entity;
///
/// struct User {
///     id: u64,
///     name: String,
/// }
///
/// impl Entity for User {
///     type Key<'a> = u64;
///     type Error = std::convert::Infallible;
///
///     fn key(&self) -> Self::Key<'_> {
///         self.id
///     }
///
///     fn to_bytes(&self) -> Result<Vec<u8>, Self::Error> {
///         Ok(self.name.as_bytes().to_vec())
///     }
///
///     fn from_bytes(bytes: &[u8]) -> Result<Self, Self::Error> {
///         Ok(Self {
///             id: 0,
///             name: String::from_utf8_lossy(bytes).into_owned(),
///         })
///     }
/// }
/// ```
pub trait Entity: Sized {
    /// The primary key type of the entity.
    ///
    /// The key may borrow from `self` to avoid allocations.
    type Key<'a>: Key
    where
        Self: 'a;

    /// Error returned when encoding or decoding the entity fails.
    ///
    /// Use your codec's native error type here, for example
    /// `serde_json::Error`, `bincode::error::DecodeError`, or an application
    /// error enum.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Returns the primary key of the entity.
    ///
    /// Implementations should prefer returning borrowed keys when possible.
    fn key(&self) -> Self::Key<'_>;

    /// Encodes the entity into storage bytes.
    ///
    /// The encoded representation is stored as the collection value inside the
    /// underlying KV store.
    fn to_bytes(&self) -> Result<Vec<u8>, Self::Error>;

    /// Decodes an entity from storage bytes.
    ///
    /// Implementations should return an error if the input bytes are malformed
    /// or incompatible with the expected entity format.
    fn from_bytes(bytes: &[u8]) -> Result<Self, Self::Error>;
}
