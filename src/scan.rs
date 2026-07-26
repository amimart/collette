//! Typed scan builders for collections and secondary indexes.
//!
//! Scans are lazy: they collect bounds, direction, and cursor information until
//! [`Scan::iter`] opens the backend stores and returns an iterator.
//!
//! Use [`Collection::scan`](crate::Collection::scan) to scan records by primary
//! key, or [`Collection::index_scan`](crate::Collection::index_scan) to scan a
//! registered secondary index. Both scan kinds can be refined with range,
//! direction, and cursor steps; index scans can also use typed prefixes.
//!
//! # Collection scans
//!
//! ```rust,ignore
//! use collette::{Cursor, Direction, Scan};
//!
//! let page = collection.scan()?
//!     .range(first_id..last_id)
//!     .direction(Direction::LeftToRight)
//!     .iter()?;
//!
//! let cursor = Cursor::from_key(last_seen_id);
//!
//! let next_page = collection.scan()?
//!     .after(cursor)
//!     .iter()?;
//! ```
//!
//! # Index scans
//!
//! ```rust,ignore
//! use collette::{Direction, Index, PrefixableScan, Scan};
//!
//! let users = collection.index_scan(ByStatusAndCreatedAt)?
//!     .prefix(Status::Active)
//!     .range(created_from..created_to)
//!     .direction(Direction::LeftToRight);
//!
//! let page = users.iter()?;
//!
//! let cursor = ByStatusAndCreatedAt::cursor(&last_seen_user);
//!
//! let next_page = collection.index_scan(ByStatusAndCreatedAt)?
//!     .prefix(Status::Active)
//!     .after(cursor)
//!     .iter()?;
//! ```

use crate::bounds::{BoundsEncoder, ExactEncoder, ScanBound, ScanRange};
use crate::error::Error;
use crate::index::{Index, IndexKind, UniqueIndexKind};
use crate::item::Item;
use crate::iter::{CollectionIterator, Cursor, IndexIterator};
use crate::key::Key;
use crate::prefix::{Prefix, Prefixable};
use crate::store::{MultiStoreReadHandle, ReadKVStore};
use std::marker::PhantomData;
use std::ops::{Bound, RangeBounds};

/// Iteration direction for range scans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Scan from the smallest encoded key to the largest.
    LeftToRight,
    /// Scan from the largest encoded key to the smallest.
    RightToLeft,
}

/// Opens a compiled scan against its backing store.
///
/// This trait separates scan construction from execution. Scan builders compile
/// down to byte bounds and a direction, then the executor opens the concrete
/// iterator for the backend.
///
/// Application code usually does not implement or call this trait directly.
/// Collection and index scans use Collette's executors internally.
pub trait ScanExecutor: Sized {
    /// Iterator produced by this executor.
    type Iter;

    /// Opens an iterator over `start..end` in `direction`.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let iter = executor.open(
    ///     std::ops::Bound::Unbounded,
    ///     std::ops::Bound::Unbounded,
    ///     Direction::LeftToRight,
    /// )?;
    /// ```
    fn open(
        self,
        start: ScanBound,
        end: ScanBound,
        direction: Direction,
    ) -> Result<Self::Iter, Error>;
}

/// A fully compiled scan ready to be executed.
///
/// A compiled scan contains encoded byte bounds, a direction, and the executor
/// that knows how to open the backend iterator.
pub struct CompiledScan<E: ScanExecutor> {
    executor: E,
    range: ScanRange,
    direction: Direction,
}

impl<E: ScanExecutor> CompiledScan<E> {
    /// Opens the scan iterator.
    ///
    /// Most callers use [`Scan::iter`] instead, which compiles and opens the
    /// scan in one step.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let compiled = users.index_scan(ByEmail)?.compile()?;
    /// let iter = compiled.iter()?;
    /// ```
    pub fn iter(self) -> Result<E::Iter, Error> {
        self.executor
            .open(self.range.0, self.range.1, self.direction)
    }
}

/// Executor used by collection index scans.
///
/// It opens the index store, applies the compiled bounds, and opens the primary
/// collection store used by [`IndexIterator`] to load records.
pub struct IndexScanExecutor<ReadHandle, Record, Idx>
where
    ReadHandle: MultiStoreReadHandle,
    Record: Item,
    Idx: Index<Record>,
    for<'b> Idx::Kind<'b>: IndexKind<Idx::Key<'b>, Record::Key<'b>>,
{
    collection_name: &'static str,
    read_handle: ReadHandle,

    _marker: PhantomData<(Record, Idx)>,
}

impl<ReadHandle, Record, Idx> ScanExecutor for IndexScanExecutor<ReadHandle, Record, Idx>
where
    ReadHandle: MultiStoreReadHandle,
    Record: Item,
    Idx: Index<Record>,
    for<'b> Idx::Kind<'b>: IndexKind<Idx::Key<'b>, Record::Key<'b>>,
{
    type Iter = IndexIterator<ReadHandle::Store, Record>;

    fn open(
        self,
        start: ScanBound,
        end: ScanBound,
        direction: Direction,
    ) -> Result<Self::Iter, Error> {
        Ok(IndexIterator::new(
            self.read_handle
                .open_store(Idx::NAME)
                .map_err(Error::backend)?
                .scan((start, end), direction)
                .map_err(Error::backend)?,
            self.read_handle
                .open_store(self.collection_name)
                .map_err(Error::backend)?,
        ))
    }
}

/// Executor used by primary collection scans.
///
/// It opens the collection's primary store, applies the compiled bounds, and
/// gives the resulting backend iterator to [`CollectionIterator`] to decode
/// records.
pub struct CollectionScanExecutor<ReadHandle, Record>
where
    ReadHandle: MultiStoreReadHandle,
    Record: Item,
{
    collection_name: &'static str,
    read_handle: ReadHandle,

    _marker: PhantomData<Record>,
}

impl<ReadHandle, Record> ScanExecutor for CollectionScanExecutor<ReadHandle, Record>
where
    ReadHandle: MultiStoreReadHandle,
    Record: Item,
{
    type Iter = CollectionIterator<ReadHandle::Store, Record>;

    fn open(
        self,
        start: ScanBound,
        end: ScanBound,
        direction: Direction,
    ) -> Result<Self::Iter, Error> {
        Ok(CollectionIterator::new(
            self.read_handle
                .open_store(self.collection_name)
                .map_err(Error::backend)?
                .scan((start, end), direction)
                .map_err(Error::backend)?,
        ))
    }
}

/// Shared contract implemented by every scan builder in the chain.
///
/// The trait keeps scan builders composable: each builder can compile itself
/// into a [`CompiledScan`], and callers can finish the chain with [`iter`](Self::iter).
///
/// # Examples
///
/// ```rust,ignore
/// use collette::{Direction, Scan};
///
/// let iter = users.scan()?
///     .direction(Direction::LeftToRight)
///     .iter()?;
/// ```
pub trait Scan: Sized {
    /// Logical key accepted by range builders.
    ///
    /// For collection scans this is the record's primary key. For index scans
    /// this is the logical index key, before any backend-specific physical key
    /// expansion.
    type Key<'a>: Key
    where
        Self: 'a;

    /// Executor used when the scan is opened.
    type Executor: ScanExecutor;

    type BoundsEncoder<'a>: BoundsEncoder<Self::Key<'a>>
    where
        Self: 'a;

    /// Compiles this builder into encoded scan bounds.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let compiled = users.scan()?.compile()?;
    /// ```
    fn compile(self) -> Result<CompiledScan<Self::Executor>, Error>;

    /// Compiles and opens this scan.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use collette::Scan;
    ///
    /// let iter = users.scan()?.iter()?;
    /// ```
    fn iter(self) -> Result<<Self::Executor as ScanExecutor>::Iter, Error> {
        self.compile()?.iter()
    }
}

/// Initial builder for a full index scan.
///
/// A full scan has no bounds and scans left-to-right by default. Use
/// [`Collection::index_scan`](crate::Collection::index_scan) to create this builder.
pub struct IndexScan<'a, ReadHandle, Record, Idx>
where
    ReadHandle: MultiStoreReadHandle,
    Record: Item,
    Idx: Index<Record>,
    for<'b> Idx::Kind<'b>: IndexKind<Idx::Key<'b>, Record::Key<'b>>,
{
    executor: IndexScanExecutor<ReadHandle, Record, Idx>,

    _marker: PhantomData<&'a ()>,
}

impl<'a, ReadHandle, Record, Idx> Scan for IndexScan<'a, ReadHandle, Record, Idx>
where
    ReadHandle: MultiStoreReadHandle,
    Record: Item,
    Idx: Index<Record>,
    for<'b> Idx::Kind<'b>: IndexKind<Idx::Key<'b>, Record::Key<'b>>,
{
    type Key<'b>
        = Idx::Key<'b>
    where
        Self: 'b;

    type Executor = IndexScanExecutor<ReadHandle, Record, Idx>;

    type BoundsEncoder<'b>
        = <Idx::Kind<'b> as IndexKind<Idx::Key<'b>, Record::Key<'b>>>::BoundsEncoder
    where
        Self: 'b;

    fn compile(self) -> Result<CompiledScan<Self::Executor>, Error> {
        Ok(CompiledScan {
            executor: self.executor,
            range: (Bound::Unbounded, Bound::Unbounded),
            direction: Direction::LeftToRight,
        })
    }
}

impl<'a, ReadHandle, Record, Idx> IndexScan<'a, ReadHandle, Record, Idx>
where
    Self: Scan,
    ReadHandle: MultiStoreReadHandle,
    Record: Item,
    Idx: Index<Record>,
    for<'b> Idx::Kind<'b>: IndexKind<Idx::Key<'b>, Record::Key<'b>>,
{
    /// Creates a full index scan from a read handle and collection store name.
    ///
    /// This constructor is primarily used by [`Collection::index_scan`](crate::Collection::index_scan).
    /// Application code should generally start scans from the collection.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let scan = users.index_scan(ByEmail)?;
    /// ```
    pub fn new(read_handle: ReadHandle, collection_name: &'static str) -> Self {
        Self {
            executor: IndexScanExecutor {
                collection_name,
                read_handle,
                _marker: Default::default(),
            },
            _marker: Default::default(),
        }
    }

    /// Restricts the scan to a range over the logical index key.
    ///
    /// For a `Multi` index, this range is over the index key defined by
    /// [`Index::Key`], not over the physical key with the
    /// appended primary key.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let active_recent = users.index_scan(ByStatusAndCreatedAt)?
    ///     .range((Status::Active, from)..(Status::Active, to))
    ///     .iter()?;
    /// ```
    pub fn range<R>(self, range: R) -> RangeScan<'a, Self>
    where
        R: RangeBounds<<Self as Scan>::Key<'a>>,
    {
        RangeScan {
            range: <<Self as Scan>::BoundsEncoder<'a> as BoundsEncoder<
                <Self as Scan>::Key<'a>,
            >>::encode_range(range),
            inner: self,
            _marker: Default::default(),
        }
    }

    /// Sets the scan direction.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use collette::{Direction, Scan};
    ///
    /// let newest_first = users.index_scan(ByCreatedAt)?
    ///     .direction(Direction::RightToLeft)
    ///     .iter()?;
    /// ```
    pub fn direction(self, direction: Direction) -> DirectedScan<'a, Self> {
        DirectedScan {
            direction,
            inner: self,
            _marker: Default::default(),
        }
    }

    /// Starts the scan after a cursor.
    ///
    /// The cursor must use the same key layout as this index. Cursors returned
    /// by index iterators already have the right shape. To build a cursor from
    /// a record, use [`Index::cursor`].
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use collette::{Index, Scan};
    ///
    /// let cursor = ByStatusAndCreatedAt::cursor(&user);
    ///
    /// let next_page = users.index_scan(ByStatusAndCreatedAt)?
    ///     .after(cursor)
    ///     .iter()?;
    /// ```
    pub fn after(self, cursor: Cursor) -> AfterScan<'a, Self> {
        AfterScan {
            cursor,
            inner: self,
            _marker: Default::default(),
        }
    }
}

impl<'a, ReadHandle, Record, Idx> IndexScan<'a, ReadHandle, Record, Idx>
where
    ReadHandle: MultiStoreReadHandle,
    Record: Item + 'a,
    Idx: Index<Record>,
    for<'b> Idx::Kind<'b>: UniqueIndexKind<Idx::Key<'b>, Record::Key<'b>>,
{
    /// Retrieves a record by unique index key.
    ///
    /// This method is only available for indexes whose [`IndexKind`] is
    /// [`Unique`](crate::Unique). It first resolves the primary key from the
    /// unique index, then loads and decodes the associated record from the
    /// collection store.
    ///
    /// Returns `Ok(None)` when no record exists for the given index key.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let user = users
    ///     .index_scan(ByEmail)?
    ///     .get("ada@example.test".to_string())?;
    /// ```
    pub fn get(
        self,
        key: impl std::borrow::Borrow<<<Idx as Index<Record>>::Key<'a> as Key>::OwnedKey>,
    ) -> Result<Option<Record>, Error> {
        let index_key = key.borrow().encode();
        let index_store = self
            .executor
            .read_handle
            .open_store(Idx::NAME)
            .map_err(Error::backend)?;
        let primary_key = index_store.get(index_key).map_err(Error::backend)?;

        let Some(primary_key) = primary_key else {
            return Ok(None);
        };

        let primary_store = self
            .executor
            .read_handle
            .open_store(self.executor.collection_name)
            .map_err(Error::backend)?;

        let record = primary_store
            .get(primary_key.as_ref())
            .map_err(Error::backend)?
            .map(|bytes| Record::from_bytes(bytes.as_ref()).map_err(Error::codec))
            .transpose()?;

        Ok(record)
    }
}

/// Initial builder for a full primary collection scan.
///
/// A collection scan reads records directly from the collection's primary
/// store, ordered by [`Item::Key`]. It scans left-to-right without bounds by
/// default. Use [`Collection::scan`](crate::Collection::scan) to create this
/// builder.
pub struct CollectionScan<'a, ReadHandle, Record>
where
    ReadHandle: MultiStoreReadHandle,
    Record: Item,
{
    executor: CollectionScanExecutor<ReadHandle, Record>,

    _marker: PhantomData<&'a ()>,
}

impl<'a, ReadHandle, Record> Scan for CollectionScan<'a, ReadHandle, Record>
where
    ReadHandle: MultiStoreReadHandle,
    Record: Item,
{
    type Key<'b>
        = Record::Key<'b>
    where
        Self: 'b;

    type Executor = CollectionScanExecutor<ReadHandle, Record>;

    type BoundsEncoder<'c>
        = ExactEncoder
    where
        Self: 'c;

    fn compile(self) -> Result<CompiledScan<Self::Executor>, Error> {
        Ok(CompiledScan {
            executor: self.executor,
            range: (Bound::Unbounded, Bound::Unbounded),
            direction: Direction::LeftToRight,
        })
    }
}

impl<'a, ReadHandle, Record> CollectionScan<'a, ReadHandle, Record>
where
    Self: Scan,
    ReadHandle: MultiStoreReadHandle,
    Record: Item,
{
    /// Creates a full collection scan from a read handle and collection store name.
    ///
    /// This constructor is primarily used by [`Collection::scan`](crate::Collection::scan).
    /// Application code should generally start scans from the collection.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let scan = users.scan()?;
    /// ```
    pub fn new(read_handle: ReadHandle, collection_name: &'static str) -> Self {
        Self {
            executor: CollectionScanExecutor {
                collection_name,
                read_handle,
                _marker: Default::default(),
            },
            _marker: Default::default(),
        }
    }

    /// Restricts the scan to a range over the primary key.
    ///
    /// The bounds are encoded with exact primary-key semantics. Unlike a
    /// `Multi` index scan, there is no appended key segment to account for.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let active_recent = users.scan()?
    ///     .range(user_id_from..user_id_to)
    ///     .iter()?;
    /// ```
    pub fn range<R>(self, range: R) -> RangeScan<'a, Self>
    where
        R: RangeBounds<<Self as Scan>::Key<'a>>,
    {
        RangeScan {
            range: <<Self as Scan>::BoundsEncoder<'a> as BoundsEncoder<
                <Self as Scan>::Key<'a>,
            >>::encode_range(range),
            inner: self,
            _marker: Default::default(),
        }
    }

    /// Sets the scan direction.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use collette::{Direction, Scan};
    ///
    /// let newest_first = users.scan()?
    ///     .direction(Direction::RightToLeft)
    ///     .iter()?;
    /// ```
    pub fn direction(self, direction: Direction) -> DirectedScan<'a, Self> {
        DirectedScan {
            direction,
            inner: self,
            _marker: Default::default(),
        }
    }

    /// Starts the scan after a cursor.
    ///
    /// The cursor must use the same layout as the primary key. Cursors returned
    /// by collection iterators already have the right shape. To build one from
    /// a primary key, use [`Cursor::from_key`].
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use collette::{Cursor, Scan};
    ///
    /// let cursor = Cursor::from_key(last_seen_id);
    ///
    /// let next_page = users.scan()?
    ///     .after(cursor)
    ///     .iter()?;
    /// ```
    pub fn after(self, cursor: Cursor) -> AfterScan<'a, Self> {
        AfterScan {
            cursor,
            inner: self,
            _marker: Default::default(),
        }
    }
}

/// Adds typed prefix support to a scan.
///
/// A prefix is a leftmost part of the scan key. For an index key
/// `(Status, CreatedAt)`, `Status` is a valid prefix and `CreatedAt` is the
/// suffix that can be ranged over afterward.
///
/// # Examples
///
/// ```rust,ignore
/// use collette::{PrefixableScan, Scan};
///
/// let active = users.index_scan(ByStatusAndCreatedAt)?
///     .prefix(Status::Active)
///     .iter()?;
/// ```
pub trait PrefixableScan<'a, K: Key + Prefixable<P>, P: Prefix>: Scan
where
    Self: 'a,
    <Self as Scan>::Key<'a>: Key + Prefixable<P>,
{
    /// Restricts the scan to keys beginning with `prefix`.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use collette::{PrefixableScan, Scan};
    ///
    /// let active = users.index_scan(ByStatusAndCreatedAt)?
    ///     .prefix(Status::Active)
    ///     .iter()?;
    /// ```
    fn prefix(self, prefix: P) -> PrefixedScan<'a, Self, P>;
}

impl<'a, ReadHandle, Record, Idx, P> PrefixableScan<'a, <Self as Scan>::Key<'a>, P>
    for IndexScan<'a, ReadHandle, Record, Idx>
where
    Self: Scan,
    <Self as Scan>::Key<'a>: Key + Prefixable<P>,
    P: Prefix,
    ReadHandle: MultiStoreReadHandle,
    Record: Item,
    Idx: Index<Record>,
    for<'b> Idx::Kind<'b>: IndexKind<Idx::Key<'b>, Record::Key<'b>>,
{
    fn prefix(self, prefix: P) -> PrefixedScan<'a, Self, P> {
        PrefixedScan {
            prefix,
            inner: self,
            _marker: Default::default(),
        }
    }
}

/// Scan builder restricted to a typed prefix.
///
/// Returned by [`PrefixableScan::prefix`]. It can be further refined with a
/// suffix range, direction, or cursor.
pub struct PrefixedScan<'a, S, P>
where
    S: Scan + 'a,
    P: Prefix,
    S::Key<'a>: Prefixable<P>,
{
    prefix: P,
    inner: S,

    _marker: PhantomData<&'a ()>,
}

impl<'a, S, P> Scan for PrefixedScan<'a, S, P>
where
    S: Scan,
    P: Prefix + 'a,
    S::Key<'a>: Prefixable<P>,
{
    type Key<'b>
        = S::Key<'b>
    where
        Self: 'b;
    type Executor = S::Executor;
    type BoundsEncoder<'b>
        = S::BoundsEncoder<'b>
    where
        Self: 'b;

    fn compile(self) -> Result<CompiledScan<Self::Executor>, Error> {
        let mut scan = self.inner.compile()?;
        scan.range = <<Self as Scan>::BoundsEncoder<'a> as BoundsEncoder<
            <Self as Scan>::Key<'a>,
        >>::encode_prefix(&self.prefix);
        Ok(scan)
    }
}

impl<'a, S, P> PrefixedScan<'a, S, P>
where
    S: Scan,
    P: Prefix,
    S::Key<'a>: Prefixable<P>,
{
    /// Restricts the prefixed scan to a range over the remaining suffix.
    ///
    /// For an index key `(Status, CreatedAt)`, calling
    /// `.prefix(Status::Active).range(from..to)` scans active records whose
    /// `CreatedAt` value falls inside `from..to`.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use collette::{PrefixableScan, Scan};
    ///
    /// let recently_active = users.index_scan(ByStatusAndCreatedAt)?
    ///     .prefix(Status::Active)
    ///     .range(created_from..created_to)
    ///     .iter()?;
    /// ```
    pub fn range<R>(self, range: R) -> RangeScan<'a, Self>
    where
        R: RangeBounds<<S::Key<'a> as Prefixable<P>>::Suffix>,
    {
        RangeScan {
            range: <S::BoundsEncoder<'a> as BoundsEncoder<S::Key<'a>>>::encode_prefixed_range(
                self.prefix.clone(),
                range,
            ),
            inner: self,
            _marker: Default::default(),
        }
    }

    /// Sets the scan direction while keeping the prefix applied.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use collette::{Direction, PrefixableScan, Scan};
    ///
    /// let newest_active = users.index_scan(ByStatusAndCreatedAt)?
    ///     .prefix(Status::Active)
    ///     .direction(Direction::RightToLeft)
    ///     .iter()?;
    /// ```
    pub fn direction(self, direction: Direction) -> DirectedScan<'a, Self> {
        DirectedScan {
            direction,
            inner: self,
            _marker: Default::default(),
        }
    }

    /// Starts the prefixed scan after a cursor.
    ///
    /// The cursor must still be inside the selected prefix.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use collette::{Index, PrefixableScan, Scan};
    ///
    /// let cursor = ByStatusAndCreatedAt::cursor(&user);
    ///
    /// let next_page = users.index_scan(ByStatusAndCreatedAt)?
    ///     .prefix(Status::Active)
    ///     .after(cursor)
    ///     .iter()?;
    /// ```
    pub fn after(self, cursor: Cursor) -> AfterScan<'a, Self> {
        AfterScan {
            cursor,
            inner: self,
            _marker: Default::default(),
        }
    }
}

/// Scan builder restricted to a typed key range.
///
/// Returned by [`IndexScan::range`] or [`PrefixedScan::range`].
pub struct RangeScan<'a, S>
where
    S: Scan + 'a,
{
    range: ScanRange,
    inner: S,

    _marker: PhantomData<&'a ()>,
}

impl<'a, S> Scan for RangeScan<'a, S>
where
    S: Scan,
{
    type Key<'b>
        = S::Key<'b>
    where
        Self: 'b;
    type Executor = S::Executor;
    type BoundsEncoder<'b>
        = S::BoundsEncoder<'b>
    where
        Self: 'b;

    fn compile(self) -> Result<CompiledScan<Self::Executor>, Error> {
        let mut scan = self.inner.compile()?;
        scan.range = self.range;
        Ok(scan)
    }
}

impl<'a, S> RangeScan<'a, S>
where
    S: Scan,
{
    /// Sets the scan direction while keeping the range applied.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use collette::{Direction, Scan};
    ///
    /// let descending = users.index_scan(ByCreatedAt)?
    ///     .range(from..to)
    ///     .direction(Direction::RightToLeft)
    ///     .iter()?;
    /// ```
    pub fn direction(self, direction: Direction) -> DirectedScan<'a, Self> {
        DirectedScan {
            direction,
            inner: self,
            _marker: Default::default(),
        }
    }

    /// Starts the ranged scan after a cursor.
    ///
    /// The cursor must fall inside the configured range.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use collette::{Index, Scan};
    ///
    /// let cursor = ByCreatedAt::cursor(&user);
    ///
    /// let next_page = users.index_scan(ByCreatedAt)?
    ///     .range(from..to)
    ///     .after(cursor)
    ///     .iter()?;
    /// ```
    pub fn after(self, cursor: Cursor) -> AfterScan<'a, Self> {
        AfterScan {
            cursor,
            inner: self,
            _marker: Default::default(),
        }
    }
}

/// Scan builder with an explicit direction.
///
/// Returned by `direction` methods on the other scan builders.
pub struct DirectedScan<'a, S> {
    direction: Direction,
    inner: S,

    _marker: PhantomData<&'a ()>,
}

impl<'a, S> Scan for DirectedScan<'a, S>
where
    S: Scan,
{
    type Key<'b>
        = S::Key<'b>
    where
        Self: 'b;
    type Executor = S::Executor;
    type BoundsEncoder<'b>
        = S::BoundsEncoder<'b>
    where
        Self: 'b;

    fn compile(self) -> Result<CompiledScan<Self::Executor>, Error> {
        let mut scan = self.inner.compile()?;
        scan.direction = self.direction;

        Ok(scan)
    }
}

impl<'a, S> DirectedScan<'a, S>
where
    S: Scan,
{
    /// Starts the directed scan after a cursor.
    ///
    /// For left-to-right scans, the cursor tightens the left bound. For
    /// right-to-left scans, it tightens the right bound.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use collette::{Direction, Index, Scan};
    ///
    /// let cursor = ByCreatedAt::cursor(&user);
    ///
    /// let previous_page = users.index_scan(ByCreatedAt)?
    ///     .direction(Direction::RightToLeft)
    ///     .after(cursor)
    ///     .iter()?;
    /// ```
    pub fn after(self, cursor: Cursor) -> AfterScan<'a, Self> {
        AfterScan {
            cursor,
            inner: self,
            _marker: Default::default(),
        }
    }
}

/// Scan builder with an encoded cursor applied.
///
/// Returned by `after` methods on the other scan builders. The cursor is
/// validated when the scan is compiled; if it falls outside the configured
/// bounds, iteration returns [`Error::CursorOutOfBounds`].
pub struct AfterScan<'a, S>
where
    S: Scan + 'a,
{
    cursor: Cursor,
    inner: S,

    _marker: PhantomData<&'a ()>,
}

impl<'a, S> Scan for AfterScan<'a, S>
where
    S: Scan,
{
    type Key<'b>
        = S::Key<'b>
    where
        Self: 'b;
    type Executor = S::Executor;
    type BoundsEncoder<'b>
        = S::BoundsEncoder<'b>
    where
        Self: 'b;

    fn compile(self) -> Result<CompiledScan<Self::Executor>, Error> {
        let mut scan = self.inner.compile()?;
        let cursor = self.cursor.into_vec();

        if !(scan.range.0.as_ref(), scan.range.1.as_ref()).contains(&cursor) {
            return Err(Error::CursorOutOfBounds);
        }

        match scan.direction {
            Direction::LeftToRight => scan.range.0 = Bound::Excluded(cursor),
            Direction::RightToLeft => scan.range.1 = Bound::Excluded(cursor),
        }

        Ok(scan)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::index::{Index, Multi, Unique};
    use crate::item::Item;
    use crate::key::Key;
    use crate::prefix::encoded_prefix_range;
    use crate::store::MultiStore;
    use crate::testing::{MockDb, MockReadHandle, ScanLog};

    #[derive(Debug, PartialEq, Eq)]
    struct Record {
        id: u32,
        group: u32,
        number: u32,
    }

    impl Item for Record {
        type Key<'a> = u32;
        type Error = std::io::Error;

        fn key(&self) -> Self::Key<'_> {
            self.id
        }

        fn to_bytes(&self) -> Result<Vec<u8>, Self::Error> {
            Ok(self.id.to_be_bytes().to_vec())
        }

        fn from_bytes(bytes: &[u8]) -> Result<Self, Self::Error> {
            let id = u32::from_be_bytes(
                bytes
                    .try_into()
                    .map_err(|_| std::io::Error::other("bad length"))?,
            );
            Ok(Self {
                id,
                group: 0,
                number: 0,
            })
        }
    }

    struct ByUniqueNumber;

    impl Index<Record> for ByUniqueNumber {
        type Key<'a> = u32;
        type Kind<'a> = Unique;

        const NAME: &'static str = "unique_number";

        fn key(item: &Record) -> Self::Key<'_> {
            item.number
        }
    }

    struct ByNumber;

    impl Index<Record> for ByNumber {
        type Key<'a> = (u32, u32);
        type Kind<'a> = Multi;

        const NAME: &'static str = "number";

        fn key(item: &Record) -> Self::Key<'_> {
            (item.group, item.number)
        }
    }

    #[test]
    fn full_scan_opens_unbounded_left_to_right() {
        assert_scan(
            |scan| scan,
            Ok(scan_log(
                Bound::Unbounded,
                Bound::Unbounded,
                Direction::LeftToRight,
            )),
        );
    }

    #[test]
    fn compiled_scan_iter_delegates_to_executor() {
        let scan = CompiledScan {
            executor: TestExecutor,
            range: (Bound::Included(vec![0x01]), Bound::Excluded(vec![0x02])),
            direction: Direction::RightToLeft,
        };

        assert_eq!(
            scan.iter().unwrap(),
            scan_log(
                Bound::Included(vec![0x01]),
                Bound::Excluded(vec![0x02]),
                Direction::RightToLeft,
            )
        );
    }

    #[test]
    fn full_scan_direction_overrides_iteration_direction() {
        assert_scan(
            |scan| scan.direction(Direction::RightToLeft),
            Ok(scan_log(
                Bound::Unbounded,
                Bound::Unbounded,
                Direction::RightToLeft,
            )),
        );
    }

    #[test]
    fn full_scan_after_tightens_left_bound() {
        assert_scan(
            |scan| scan.after(Cursor::from_key((2u32, 20u32, 200u32))),
            Ok(scan_log(
                Bound::Excluded(encode_store_key(2, 20, 200)),
                Bound::Unbounded,
                Direction::LeftToRight,
            )),
        );
    }

    #[test]
    fn unique_index_scan_get_loads_record_by_index_key() {
        let primary_key = encode_primary_key(7);
        let record = Record {
            id: 7,
            group: 2,
            number: 99,
        };
        let db = MockDb::new()
            .with_data(ByUniqueNumber::NAME, 99u32.encode(), primary_key.clone())
            .with_data("records", primary_key.clone(), record.to_bytes().unwrap());
        let log = db.log();
        let read = db.read("records").unwrap();

        let found = IndexScan::<_, Record, ByUniqueNumber>::new(read, "records")
            .get(99u32)
            .unwrap();

        assert_eq!(
            found,
            Some(Record {
                id: 7,
                group: 0,
                number: 0
            })
        );
        assert_eq!(log.borrow().opens, vec![ByUniqueNumber::NAME, "records"]);
        assert_eq!(log.borrow().gets, vec![encode_primary_key(99), primary_key]);
    }

    #[test]
    fn unique_index_scan_get_returns_none_when_index_key_is_missing() {
        let db = MockDb::new();
        let log = db.log();
        let read = db.read("records").unwrap();

        let found = IndexScan::<_, Record, ByUniqueNumber>::new(read, "records")
            .get(99u32)
            .unwrap();

        assert_eq!(found, None);
        assert_eq!(log.borrow().opens, vec![ByUniqueNumber::NAME]);
        assert_eq!(log.borrow().gets, vec![encode_primary_key(99)]);
    }

    #[test]
    fn range_scan_encodes_logical_index_key_bounds() {
        assert_scan(
            |scan| scan.range((1u32, 10u32)..(3u32, 30u32)),
            Ok(scan_log(
                Bound::Included(encode_index_key(1, 10)),
                Bound::Excluded(encode_index_key(3, 30)),
                Direction::LeftToRight,
            )),
        );
    }

    #[test]
    fn range_scan_direction_overrides_iteration_direction() {
        assert_scan(
            |scan| {
                scan.range((1u32, 10u32)..(3u32, 30u32))
                    .direction(Direction::RightToLeft)
            },
            Ok(scan_log(
                Bound::Included(encode_index_key(1, 10)),
                Bound::Excluded(encode_index_key(3, 30)),
                Direction::RightToLeft,
            )),
        );
    }

    #[test]
    fn prefix_scan_encodes_prefix_bounds() {
        assert_scan(
            |scan| scan.prefix(2u32),
            Ok(scan_log(
                encoded_prefix_range(encode_prefix(2)).0,
                encoded_prefix_range(encode_prefix(2)).1,
                Direction::LeftToRight,
            )),
        );
    }

    #[test]
    fn prefixed_scan_after_tightens_left_bound() {
        assert_scan(
            |scan| {
                scan.prefix(2u32)
                    .after(Cursor::from_key((2u32, 20u32, 200u32)))
            },
            Ok(scan_log(
                Bound::Excluded(encode_store_key(2, 20, 200)),
                encoded_prefix_range(encode_prefix(2)).1,
                Direction::LeftToRight,
            )),
        );
    }

    #[test]
    fn prefixed_range_composes_prefix_and_finite_suffix_bounds() {
        assert_scan(
            |scan| scan.prefix(2u32).range(20u32..30u32),
            Ok(scan_log(
                Bound::Included(encode_index_key(2, 20)),
                Bound::Excluded(encode_index_key(2, 30)),
                Direction::LeftToRight,
            )),
        );
    }

    #[test]
    fn prefixed_range_accepts_unbounded_suffix_range() {
        assert_scan(
            |scan| scan.prefix(2u32).range(..),
            Ok(scan_log(
                encoded_prefix_range(encode_prefix(2)).0,
                encoded_prefix_range(encode_prefix(2)).1,
                Direction::LeftToRight,
            )),
        );
    }

    #[test]
    fn direction_overrides_iteration_direction() {
        assert_scan(
            |scan| scan.prefix(2u32).direction(Direction::RightToLeft),
            Ok(scan_log(
                encoded_prefix_range(encode_prefix(2)).0,
                encoded_prefix_range(encode_prefix(2)).1,
                Direction::RightToLeft,
            )),
        );
    }

    #[test]
    fn directed_scan_after_tightens_left_bound() {
        assert_scan(
            |scan| {
                scan.range((1u32, 10u32)..(3u32, 30u32))
                    .direction(Direction::LeftToRight)
                    .after(Cursor::from_key((2u32, 20u32, 200u32)))
            },
            Ok(scan_log(
                Bound::Excluded(encode_store_key(2, 20, 200)),
                Bound::Excluded(encode_index_key(3, 30)),
                Direction::LeftToRight,
            )),
        );
    }

    #[test]
    fn left_to_right_cursor_tightens_left_bound() {
        assert_scan(
            |scan| {
                scan.range((1u32, 10u32)..(3u32, 30u32))
                    .after(Cursor::from_key((2u32, 20u32, 200u32)))
            },
            Ok(scan_log(
                Bound::Excluded(encode_store_key(2, 20, 200)),
                Bound::Excluded(encode_index_key(3, 30)),
                Direction::LeftToRight,
            )),
        );
    }

    #[test]
    fn cursor_on_excluded_left_bound_fails_before_opening_stores() {
        assert_scan(
            |scan| {
                scan.range((
                    Bound::Excluded((1u32, 10u32)),
                    Bound::Included((3u32, 30u32)),
                ))
                .after(Cursor::from_key((1u32, 10u32)))
            },
            Err(ErrorKind::CursorOutOfBounds),
        );
    }

    #[test]
    fn cursor_on_excluded_right_bound_fails_before_opening_stores() {
        assert_scan(
            |scan| {
                scan.range((1u32, 10u32)..(3u32, 30u32))
                    .direction(Direction::RightToLeft)
                    .after(Cursor::from_key((3u32, 30u32)))
            },
            Err(ErrorKind::CursorOutOfBounds),
        );
    }

    #[test]
    fn right_to_left_cursor_tightens_right_bound() {
        assert_scan(
            |scan| {
                scan.range((1u32, 10u32)..(3u32, 30u32))
                    .direction(Direction::RightToLeft)
                    .after(Cursor::from_key((2u32, 20u32, 200u32)))
            },
            Ok(scan_log(
                Bound::Included(encode_index_key(1, 10)),
                Bound::Excluded(encode_store_key(2, 20, 200)),
                Direction::RightToLeft,
            )),
        );
    }

    #[test]
    fn cursor_outside_bounds_fails_before_opening_stores() {
        assert_scan(
            |scan| {
                scan.prefix(2u32)
                    .after(Cursor::from_key((3u32, 20u32, 200u32)))
            },
            Err(ErrorKind::CursorOutOfBounds),
        );
    }

    #[test]
    fn collection_scan_opens_unbounded_left_to_right() {
        assert_collection_scan(
            |scan| scan,
            Ok(scan_log(
                Bound::Unbounded,
                Bound::Unbounded,
                Direction::LeftToRight,
            )),
        );
    }

    #[test]
    fn collection_scan_range_encodes_primary_key_bounds() {
        assert_collection_scan(
            |scan| scan.range(10u32..20u32),
            Ok(scan_log(
                Bound::Included(encode_primary_key(10)),
                Bound::Excluded(encode_primary_key(20)),
                Direction::LeftToRight,
            )),
        );
    }

    #[test]
    fn collection_scan_direction_overrides_iteration_direction() {
        assert_collection_scan(
            |scan| scan.direction(Direction::RightToLeft),
            Ok(scan_log(
                Bound::Unbounded,
                Bound::Unbounded,
                Direction::RightToLeft,
            )),
        );
    }

    #[test]
    fn collection_scan_after_tightens_left_bound() {
        assert_collection_scan(
            |scan| scan.after(Cursor::from_key(10u32)),
            Ok(scan_log(
                Bound::Excluded(encode_primary_key(10)),
                Bound::Unbounded,
                Direction::LeftToRight,
            )),
        );
    }

    #[test]
    fn collection_scan_right_to_left_after_tightens_right_bound() {
        assert_collection_scan(
            |scan| {
                scan.direction(Direction::RightToLeft)
                    .after(Cursor::from_key(10u32))
            },
            Ok(scan_log(
                Bound::Unbounded,
                Bound::Excluded(encode_primary_key(10)),
                Direction::RightToLeft,
            )),
        );
    }

    #[test]
    fn collection_scan_cursor_outside_bounds_fails_before_opening_stores() {
        assert_collection_scan(
            |scan| scan.range(10u32..20u32).after(Cursor::from_key(30u32)),
            Err(ErrorKind::CursorOutOfBounds),
        );
    }

    fn assert_scan<S>(
        build: impl FnOnce(IndexScan<'static, MockReadHandle, Record, ByNumber>) -> S,
        expected: Result<ScanLog, ErrorKind>,
    ) where
        S: Scan<Executor = IndexScanExecutor<MockReadHandle, Record, ByNumber>>,
    {
        let db = MockDb::new();
        let log = db.log();
        let read = db.read("records").unwrap();
        let scan = build(IndexScan::<_, Record, ByNumber>::new(read, "records"));

        match expected {
            Ok(expected) => {
                scan.iter().unwrap();
                assert_eq!(log.borrow().scans, vec![expected]);
            }
            Err(ErrorKind::CursorOutOfBounds) => {
                assert!(matches!(scan.iter(), Err(Error::CursorOutOfBounds)));
                assert!(log.borrow().scans.is_empty());
            }
        }
    }

    fn assert_collection_scan<S>(
        build: impl FnOnce(CollectionScan<'static, MockReadHandle, Record>) -> S,
        expected: Result<ScanLog, ErrorKind>,
    ) where
        S: Scan<Executor = CollectionScanExecutor<MockReadHandle, Record>>,
    {
        let db = MockDb::new();
        let log = db.log();
        let read = db.read("records").unwrap();
        let scan = build(CollectionScan::<_, Record>::new(read, "records"));

        match expected {
            Ok(expected) => {
                scan.iter().unwrap();
                assert_eq!(log.borrow().scans, vec![expected]);
            }
            Err(ErrorKind::CursorOutOfBounds) => {
                assert!(matches!(scan.iter(), Err(Error::CursorOutOfBounds)));
                assert!(log.borrow().scans.is_empty());
            }
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    enum ErrorKind {
        CursorOutOfBounds,
    }

    fn scan_log(left: Bound<Vec<u8>>, right: Bound<Vec<u8>>, direction: Direction) -> ScanLog {
        ScanLog {
            left,
            right,
            direction,
        }
    }

    fn encode_prefix(group: u32) -> Vec<u8> {
        group.encode().as_ref().to_vec()
    }

    fn encode_primary_key(id: u32) -> Vec<u8> {
        id.encode().as_ref().to_vec()
    }

    fn encode_index_key(group: u32, number: u32) -> Vec<u8> {
        (group, number).encode().as_ref().to_vec()
    }

    fn encode_store_key(group: u32, number: u32, pk: u32) -> Vec<u8> {
        (group, number, pk).encode().as_ref().to_vec()
    }

    struct TestExecutor;

    impl ScanExecutor for TestExecutor {
        type Iter = ScanLog;

        fn open(
            self,
            start: ScanBound,
            end: ScanBound,
            direction: Direction,
        ) -> Result<Self::Iter, Error> {
            Ok(scan_log(start, end, direction))
        }
    }
}
