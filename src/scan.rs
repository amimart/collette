//! Typed scan builders for secondary indexes.
//!
//! Scans are lazy: they collect bounds, direction, and cursor information until
//! [`Scan::iter`] opens the backend stores and returns an iterator.
//!
//! A scan starts from [`Collection::scan`](crate::Collection::index_scan), then can be
//! refined with range, prefix, direction, and cursor steps:
//!
//! ```rust,ignore
//! use collette::{Direction, Key, PrefixableScan, Scan};
//!
//! let users = collection.scan(ByStatusAndCreatedAt)?
//!     .prefix(Status::Active)
//!     .range(created_from..created_to)
//!     .direction(Direction::LeftToRight);
//!
//! let page = users.iter()?;
//!
//! let cursor = (Status::Active, created_at, &user_id)
//!     .encode()
//!     .as_ref()
//!     .to_vec();
//!
//! let next_page = collection.scan(ByStatusAndCreatedAt)?
//!     .prefix(Status::Active)
//!     .after(cursor)
//!     .iter()?;
//! ```

use crate::bounds::{BoundsEncoder, ScanBound, ScanRange};
use crate::error::Error;
use crate::index::{Index, IndexKind};
use crate::item::Item;
use crate::iter::IndexIterator;
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
/// Collection scans use Collette's index executor internally.
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
    /// let compiled = users.scan(ByEmail)?.compile()?;
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
/// let iter = users.scan(ByEmail)?
///     .direction(Direction::LeftToRight)
///     .iter()?;
/// ```
pub trait Scan: Sized {
    /// Logical index key accepted by range builders.
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
    /// let compiled = users.scan(ByEmail)?.compile()?;
    /// ```
    fn compile(self) -> Result<CompiledScan<Self::Executor>, Error>;

    /// Compiles and opens this scan.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use collette::Scan;
    ///
    /// let iter = users.scan(ByEmail)?.iter()?;
    /// ```
    fn iter(self) -> Result<<Self::Executor as ScanExecutor>::Iter, Error> {
        self.compile()?.iter()
    }
}

/// Initial builder for a full index scan.
///
/// A full scan has no bounds and scans left-to-right by default. Use
/// [`Collection::scan`](crate::Collection::index_scan) to create this builder.
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
    /// This constructor is primarily used by [`Collection::scan`](crate::Collection::index_scan).
    /// Application code should generally start scans from the collection.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let scan = users.scan(ByEmail)?;
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
    /// let active_recent = users.scan(ByStatusAndCreatedAt)?
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
    /// let newest_first = users.scan(ByCreatedAt)?
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

    /// Starts the scan after an encoded cursor key.
    ///
    /// The cursor must be encoded with the same key layout used by the index
    /// store. For a `Multi` index, this is typically the index key followed by
    /// the primary key.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use collette::{Key, Scan};
    ///
    /// let cursor = (Status::Active, created_at, &user_id)
    ///     .encode()
    ///     .as_ref()
    ///     .to_vec();
    ///
    /// let next_page = users.scan(ByStatusAndCreatedAt)?
    ///     .after(cursor)
    ///     .iter()?;
    /// ```
    pub fn after(self, cursor: Vec<u8>) -> AfterScan<'a, Self> {
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
/// let active = users.scan(ByStatusAndCreatedAt)?
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
    /// let active = users.scan(ByStatusAndCreatedAt)?
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
    /// let recently_active = users.scan(ByStatusAndCreatedAt)?
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
    /// let newest_active = users.scan(ByStatusAndCreatedAt)?
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

    /// Starts the prefixed scan after an encoded cursor key.
    ///
    /// The cursor must still be inside the selected prefix.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use collette::{Key, PrefixableScan, Scan};
    ///
    /// let cursor = (Status::Active, created_at, &user_id)
    ///     .encode()
    ///     .as_ref()
    ///     .to_vec();
    ///
    /// let next_page = users.scan(ByStatusAndCreatedAt)?
    ///     .prefix(Status::Active)
    ///     .after(cursor)
    ///     .iter()?;
    /// ```
    pub fn after(self, cursor: Vec<u8>) -> AfterScan<'a, Self> {
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
    /// let descending = users.scan(ByCreatedAt)?
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

    /// Starts the ranged scan after an encoded cursor key.
    ///
    /// The cursor must fall inside the configured range.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use collette::{Key, Scan};
    ///
    /// let cursor = (created_at, &user_id).encode().as_ref().to_vec();
    ///
    /// let next_page = users.scan(ByCreatedAt)?
    ///     .range(from..to)
    ///     .after(cursor)
    ///     .iter()?;
    /// ```
    pub fn after(self, cursor: Vec<u8>) -> AfterScan<'a, Self> {
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
    /// Starts the directed scan after an encoded cursor key.
    ///
    /// For left-to-right scans, the cursor tightens the left bound. For
    /// right-to-left scans, it tightens the right bound.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use collette::{Direction, Key, Scan};
    ///
    /// let cursor = (created_at, &user_id).encode().as_ref().to_vec();
    ///
    /// let previous_page = users.scan(ByCreatedAt)?
    ///     .direction(Direction::RightToLeft)
    ///     .after(cursor)
    ///     .iter()?;
    /// ```
    pub fn after(self, cursor: Vec<u8>) -> AfterScan<'a, Self> {
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
    cursor: Vec<u8>,
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

        if !(scan.range.0.as_ref(), scan.range.1.as_ref()).contains(&self.cursor) {
            return Err(Error::CursorOutOfBounds);
        }

        match scan.direction {
            Direction::LeftToRight => scan.range.0 = Bound::Excluded(self.cursor),
            Direction::RightToLeft => scan.range.1 = Bound::Excluded(self.cursor),
        }

        Ok(scan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{Index, Multi};
    use crate::item::Item;
    use crate::key::Key;
    use crate::prefix::encoded_prefix_range;
    use crate::store::MultiStore;
    use crate::testing::{MockDb, MockReadHandle, ScanLog};

    #[derive(Debug)]
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
            Ok(vec![])
        }

        fn from_bytes(_: &[u8]) -> Result<Self, Self::Error> {
            Ok(Self {
                id: 0,
                group: 0,
                number: 0,
            })
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
            |scan| scan.after(encode_store_key(2, 20, 200)),
            Ok(scan_log(
                Bound::Excluded(encode_store_key(2, 20, 200)),
                Bound::Unbounded,
                Direction::LeftToRight,
            )),
        );
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
            |scan| scan.prefix(2u32).after(encode_store_key(2, 20, 200)),
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
                    .after(encode_store_key(2, 20, 200))
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
                    .after(encode_store_key(2, 20, 200))
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
                .after(encode_index_key(1, 10))
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
                    .after(encode_index_key(3, 30))
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
                    .after(encode_store_key(2, 20, 200))
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
            |scan| scan.prefix(2u32).after(encode_store_key(3, 20, 200)),
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
