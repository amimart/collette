use crate::key::Key;

/// A persistable record stored inside a Collette collection.
///
/// An item defines:
///
/// - its primary key type;
/// - how its primary key is accessed;
/// - how it is encoded and decoded from storage bytes;
/// - which error type its codec returns.
///
/// # Examples
///
/// ```
/// use collette::Item;
///
/// struct User {
///     id: u64,
///     name: String,
/// }
///
/// impl Item for User {
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
pub trait Item: Sized {
    /// The primary key type of the item.
    ///
    /// The key may borrow from `self` to avoid allocations.
    type Key<'a>: Key
    where
        Self: 'a;

    /// Error returned when encoding or decoding the item fails.
    ///
    /// Use your codec's native error type here, for example
    /// `serde_json::Error`, `bincode::error::DecodeError`, or an application
    /// error enum. Collection operations type-erase this into
    /// [`Error::Codec`](crate::Error::Codec) when returning a Collette error.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Returns the primary key of the item.
    ///
    /// Implementations should prefer returning borrowed keys when possible.
    fn key(&self) -> Self::Key<'_>;

    /// Encodes the item into storage bytes.
    ///
    /// The encoded representation is stored as the collection value inside the
    /// underlying KV store.
    fn to_bytes(&self) -> Result<Vec<u8>, Self::Error>;

    /// Decodes an item from storage bytes.
    ///
    /// Implementations should return an error if the input bytes are malformed
    /// or incompatible with the expected item format.
    fn from_bytes(bytes: &[u8]) -> Result<Self, Self::Error>;
}
