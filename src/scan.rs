//! Typed scan builders for secondary indexes.
//!
//! Scans are lazy: they collect bounds, direction, and cursor information until
//! [`IndexScan::iter`] opens the backend stores and returns an iterator.

use crate::bounds::{prefix_range, IntoScanBounds, ScanBound, ScanRange};
use crate::error::Error;
use crate::index::{Index, IndexKind, StoreKey};
use crate::item::Item;
use crate::iter::IndexIterator;
use crate::key::Key;
use crate::prefix::{Prefix, PrefixOrKey, Prefixable};
use crate::store::{MultiStoreReadHandle, ReadKVStore};
use std::marker::PhantomData;
use std::ops::{Bound, Range, RangeBounds};

/// Iteration direction for range scans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Scan from the smallest encoded key to the largest.
    LeftToRight,
    /// Scan from the largest encoded key to the smallest.
    RightToLeft,
}

pub trait ScanExecutor: Sized {
    type Iter;

    fn open(
        self,
        start: ScanBound,
        end: ScanBound,
        direction: Direction,
    ) -> Result<Self::Iter, Error>;
}

pub struct CompiledScan<E: ScanExecutor> {
    executor: E,
    range: ScanRange,
    direction: Direction,
}

impl<E: ScanExecutor> CompiledScan<E> {
    pub fn iter(self) -> Result<E::Iter, Error> {
        self.executor.open(self.range.0, self.range.1, self.direction)
    }
}

pub struct IndexExecutor<ReadHandle, Record, Idx>
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

impl<ReadHandle, Record, Idx> ScanExecutor for IndexExecutor<ReadHandle, Record, Idx>
where
    ReadHandle: MultiStoreReadHandle,
    Record: Item,
    Idx: Index<Record>,
    for<'b> Idx::Kind<'b>: IndexKind<Idx::Key<'b>, Record::Key<'b>>,
{
    type Iter = IndexIterator<ReadHandle::Store, Record>;

    fn open(self, start: ScanBound, end: ScanBound, direction: Direction) -> Result<Self::Iter, Error> {
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

pub trait Scan: Sized {
    type Key<'a>: Key
    where
        Self: 'a;
    type Executor: ScanExecutor;

    fn compile(self) -> Result<CompiledScan<Self::Executor>, Error>;

    fn iter(self) -> Result<<Self::Executor as ScanExecutor>::Iter, Error> {
        self.compile()?.iter()
    }
}

pub struct IndexFullScan<'a, ReadHandle, Record, Idx>
where
    ReadHandle: MultiStoreReadHandle,
    Record: Item,
    Idx: Index<Record>,
    for<'b> Idx::Kind<'b>: IndexKind<Idx::Key<'b>, Record::Key<'b>>,
{
    executor: IndexExecutor<ReadHandle, Record, Idx>,

    _marker: PhantomData<&'a ()>,
}

impl<'a, ReadHandle, Record, Idx> Scan for IndexFullScan<'a, ReadHandle, Record, Idx>
where
    ReadHandle: MultiStoreReadHandle,
    Record: Item,
    Idx: Index<Record>,
    for<'b> Idx::Kind<'b>: IndexKind<Idx::Key<'b>, Record::Key<'b>>,
{
    type Key<'b> = Idx::Key<'b>
    where
        Self: 'b;
    type Executor = IndexExecutor<ReadHandle, Record, Idx>;

    fn compile(self) -> Result<CompiledScan<Self::Executor>, Error> {
        Ok(CompiledScan {
            executor: self.executor,
            range: (Bound::Unbounded, Bound::Unbounded),
            direction: Direction::LeftToRight,
        })
    }
}

impl<'a, ReadHandle, Record, Idx> IndexFullScan<'a, ReadHandle, Record, Idx>
where
    Self: Scan,
    ReadHandle: MultiStoreReadHandle,
    Record: Item,
    Idx: Index<Record>,
    for<'b> Idx::Kind<'b>: IndexKind<Idx::Key<'b>, Record::Key<'b>>,
{
    pub fn new(read_handle: ReadHandle, collection_name: &'static str) -> Self {
        Self {
            executor: IndexExecutor {
                collection_name,
                read_handle,
                _marker: Default::default(),
            },
            _marker: Default::default(),
        }
    }

    pub fn prefix<P>(self, prefix: P) -> PrefixedScan<'a, Self, P>
    where
        P: Prefix,
        <Self as Scan>::Key<'a>: Prefixable<P>,
    {
        PrefixedScan {
            prefix,
            inner: self,
            _marker: Default::default(),
        }
    }

    pub fn range<R>(self, range: R) -> RangeScan<'a, Self, R>
    where
        R: RangeBounds<<Self as Scan>::Key<'a>>,
    {
        RangeScan {
            range,
            inner: self,
            _marker: Default::default(),
        }
    }

    pub fn direction(self, direction: Direction) -> DirectedScan<'a, Self> {
        DirectedScan {
            direction,
            inner: self,
            _marker: Default::default(),
        }
    }

    pub fn after(self, cursor: Vec<u8>) -> AfterScan<'a, Self> {
        AfterScan {
            cursor,
            inner: self,
            _marker: Default::default(),
        }
    }
}

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
    P: Prefix,
    S::Key<'a>: Prefixable<P>,
{
    type Key<'b> = S::Key<'b>
    where
        Self: 'b;
    type Executor = S::Executor;

    fn compile(self) -> Result<CompiledScan<Self::Executor>, Error> {
        let mut scan = self.inner.compile()?;
        scan.range = self.prefix.range();
        Ok(scan)
    }
}

pub struct RangeScan<'a, S, R>
where
    S: Scan + 'a,
    R: RangeBounds<S::Key<'a>>,
{
    range: R,
    inner: S,

    _marker: PhantomData<&'a ()>,
}

impl<'a, S, R> Scan for RangeScan<'a, S, R>
where
    S: Scan,
    R: RangeBounds<S::Key<'a>>,
{
    type Key<'b> = S::Key<'b>
    where
        Self: 'b;
    type Executor = S::Executor;

    fn compile(self) -> Result<CompiledScan<Self::Executor>, Error> {
        let mut scan = self.inner.compile()?;
        scan.range = (
            self.range.start_bound(),
            self.range.end_bound(),
        ).range();
        Ok(scan)
    }
}

pub struct SuffixRangeScan<'a, S, P>
where
    S: Scan,
    P: Prefix,
    S::Key<'a>: Prefixable<P>,
{
    range: Range<Bound<<S::Key<'a> as Prefixable<P>>::Suffix>>,
    inner: PrefixedScan<'a, S, P>,
}

impl<'a, S, P> Scan for SuffixRangeScan<'a, S, P>
where
    S: Scan,
    P: Prefix + 'a,
    S::Key<'a>: Prefixable<P>,
{
    type Key<'b> = S::Key<'b>
    where
        Self: 'b;
    type Executor = S::Executor;

    fn compile(self) -> Result<CompiledScan<Self::Executor>, Error> {
        let prefix = self.inner.prefix.clone();

        let mut scan = self.inner.compile()?;
        scan.range = prefix_range::<Self::Key<'a>, P, <Self::Key<'a> as Prefixable<P>>::Suffix>(
            prefix,
            self.range.clone(),
        ).range();

        Ok(scan)
    }
}

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
    type Key<'b> = S::Key<'b>
    where
        Self: 'b;
    type Executor = S::Executor;

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

pub struct DirectedScan<'a, S> {
    direction: Direction,
    inner: S,

    _marker: PhantomData<&'a ()>,
}

impl<'a, S> Scan for DirectedScan<'a, S>
where
    S: Scan,
{
    type Key<'b> = S::Key<'b>
    where
        Self: 'b;
    type Executor = S::Executor;

    fn compile(self) -> Result<CompiledScan<Self::Executor>, Error> {
        let mut scan = self.inner.compile()?;
        scan.direction = self.direction;

        Ok(scan)
    }
}

/// Lazy builder for scanning a collection index.
///
/// Use [`Collection::scan`](crate::Collection::scan) to create one, then add
/// range, prefix, direction, or cursor constraints before calling
/// [`iter`](Self::iter).
pub struct IndexScan<'a, ReadHandle, Record, Idx>
where
    Self: 'a,
    ReadHandle: MultiStoreReadHandle,
    Record: Item,
    Idx: Index<Record>,
    for<'b> Idx::Kind<'b>: IndexKind<Idx::Key<'b>, Record::Key<'b>>,
{
    collection_name: &'static str,
    read_handle: ReadHandle,
    left: ScanBound,
    right: ScanBound,
    direction: Direction,
    after: Option<StoreKey<'a, 'a, Idx, Record::Key<'a>, Record>>,

    _marker: PhantomData<(Record, Idx)>,
}

impl<'a, ReadHandle, Record, Idx> IndexScan<'a, ReadHandle, Record, Idx>
where
    ReadHandle: MultiStoreReadHandle,
    Record: Item,
    Idx: Index<Record>,
    for<'b> Idx::Kind<'b>: IndexKind<Idx::Key<'b>, Record::Key<'b>>,
{
    /// Creates an unconstrained index scan.
    ///
    /// This is primarily used by [`Collection::scan`](crate::Collection::scan).
    pub fn new(collection_name: &'static str, read_handle: ReadHandle) -> Self {
        Self {
            collection_name,
            read_handle,
            left: Bound::Unbounded,
            right: Bound::Unbounded,
            direction: Direction::LeftToRight,
            after: None,

            _marker: PhantomData,
        }
    }

    /// Restricts the scan to exact physical index-key bounds.
    ///
    /// Prefer the [`PrefixScan`] methods when scanning by a typed prefix.
    pub fn range(
        mut self,
        range: Range<Bound<StoreKey<'a, 'a, Idx, Record::Key<'a>, Record>>>,
    ) -> Self {
        self.left = range.start.map(|p| p.encode().as_ref().to_vec());
        self.right = range.end.map(|p| p.encode().as_ref().to_vec());
        self
    }

    /// Sets the scan direction.
    pub fn direction(mut self, direction: Direction) -> Self {
        self.direction = direction;
        self
    }

    /// Starts the scan after the given physical index key.
    ///
    /// The cursor must fall inside the configured scan bounds.
    pub fn after(mut self, cursor: StoreKey<'a, 'a, Idx, Record::Key<'a>, Record>) -> Self {
        self.after = Some(cursor);
        self
    }

    /// Opens the backend stores and returns an iterator over matching records.
    pub fn iter(self) -> Result<IndexIterator<ReadHandle::Store, Record>, Error> {
        let (left, right) = match self.after {
            Some(cursor) => Self::apply_cursor(
                self.left,
                self.right,
                self.direction,
                cursor.encode().as_ref().to_vec(),
            )?,
            None => (self.left, self.right),
        };

        Ok(IndexIterator::new(
            self.read_handle
                .open_store(Idx::NAME)
                .map_err(Error::backend)?
                .scan((left, right), self.direction)
                .map_err(Error::backend)?,
            self.read_handle
                .open_store(self.collection_name)
                .map_err(Error::backend)?,
        ))
    }

    fn apply_cursor(
        left: ScanBound,
        right: ScanBound,
        direction: Direction,
        after: Vec<u8>,
    ) -> Result<ScanRange, Error> {
        if !(left.as_ref(), right.as_ref()).contains(&after) {
            return Err(Error::CursorOutOfBounds);
        }

        Ok(match direction {
            Direction::LeftToRight => (Bound::Excluded(after), right),
            Direction::RightToLeft => (left, Bound::Excluded(after)),
        })
    }
}

/// Prefix scanning support for composite index keys.
///
/// This trait is implemented when the stored index key can be constrained by
/// the supplied prefix type. For example, an index key `(Status, u64, &Id)` can
/// be scanned with a `Status` prefix or a `(Status, u64)` prefix.
pub trait PrefixScan<StoredKey: Key + Prefixable<KeyPrefix>, KeyPrefix: Prefix> {
    /// Restricts the scan to all keys beginning with `prefix`.
    fn prefix(self, prefix: KeyPrefix) -> Self;

    /// Restricts the scan to a range of typed prefixes.
    fn prefix_range(self, range: Range<Bound<KeyPrefix>>) -> Self;

    /// Restricts the scan using either whole keys or prefixes as endpoints.
    fn range(self, range: Range<Bound<PrefixOrKey<StoredKey, KeyPrefix>>>) -> Self;
}

impl<'a, ReadHandle, Record, Idx, KeyPrefix>
    PrefixScan<StoreKey<'a, 'a, Idx, Record::Key<'a>, Record>, KeyPrefix>
    for IndexScan<'a, ReadHandle, Record, Idx>
where
    ReadHandle: MultiStoreReadHandle,
    Record: Item,
    Idx: Index<Record>,
    KeyPrefix: Prefix,
    StoreKey<'a, 'a, Idx, Record::Key<'a>, Record>: Key + Prefixable<KeyPrefix>,
    for<'b> Idx::Kind<'b>: IndexKind<Idx::Key<'b>, Record::Key<'b>>,
{
    fn prefix(mut self, prefix: KeyPrefix) -> Self {
        self.left = prefix.start_bound();
        self.right = prefix.end_bound();
        self
    }

    fn prefix_range(mut self, range: Range<Bound<KeyPrefix>>) -> Self {
        self.left = range.start.start_bound();
        self.right = range.end.end_bound();
        self
    }

    fn range(
        mut self,
        range: Range<Bound<PrefixOrKey<StoreKey<'a, 'a, Idx, Record::Key<'a>, Record>, KeyPrefix>>>,
    ) -> Self {
        self.left = range.start.start_bound();
        self.right = range.end.end_bound();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::index::{Index, Multi};
    use crate::item::Item;
    use crate::key::Key;
    use crate::prefix::encoded_prefix_range;
    use crate::store::MultiStore;
    use crate::testing::{MockDb, ScanLog};

    #[derive(Debug)]
    struct Record {
        id: u32,
        indexed: u32,
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
            Ok(Self { id: 0, indexed: 0 })
        }
    }

    struct ByNumber;

    impl Index<Record> for ByNumber {
        type Key<'a> = (u32,);
        type Kind<'a> = Multi;
        const NAME: &'static str = "number";

        fn key(item: &Record) -> Self::Key<'_> {
            (item.indexed,)
        }
    }

    struct ScanCase {
        name: &'static str,
        setup: ScanSetup,
        direction: Direction,
        after: Option<(u32, u32)>,
        expected: Result<ScanLog, ErrorKind>,
    }

    enum ScanSetup {
        Default,
        Range {
            left: Bound<Vec<u8>>,
            right: Bound<Vec<u8>>,
        },
        Prefix(u32),
        PrefixRange {
            left: Bound<u32>,
            right: Bound<u32>,
        },
        PrefixOrKeyRange {
            left: Bound<RangeEndpoint>,
            right: Bound<RangeEndpoint>,
        },
    }

    enum RangeEndpoint {
        Prefix(u32),
        Key(u32, u32),
    }

    #[derive(Debug, PartialEq, Eq)]
    enum ErrorKind {
        CursorOutOfBounds,
    }

    impl ScanCase {
        fn assert(self) {
            let db = MockDb::new();
            let log = db.log();
            let read = db.read("records").unwrap();

            let scan = IndexScan::<_, Record, ByNumber>::new("records", read);
            let scan = match self.setup {
                ScanSetup::Default => scan,
                ScanSetup::Range { left, right } => {
                    scan.range(left.map(decode_store_key)..right.map(decode_store_key))
                }
                ScanSetup::Prefix(prefix) => scan.prefix(prefix),
                ScanSetup::PrefixRange { left, right } => scan.prefix_range(left..right),
                ScanSetup::PrefixOrKeyRange { left, right } => {
                    PrefixScan::<TestStoreKey, u32>::range(
                        scan,
                        left.map(prefix_or_key)..right.map(prefix_or_key),
                    )
                }
            }
            .direction(self.direction);
            let scan = match self.after {
                Some((index, pk)) => scan.after(store_key(index, pk)),
                None => scan,
            };

            let result = scan.iter();

            match self.expected {
                Ok(expected) => {
                    result.unwrap();
                    assert_eq!(log.borrow().scans, vec![expected], "{}", self.name);
                }
                Err(ErrorKind::CursorOutOfBounds) => {
                    assert!(
                        matches!(result, Err(Error::CursorOutOfBounds)),
                        "{}",
                        self.name
                    );
                    assert!(log.borrow().scans.is_empty(), "{}", self.name);
                }
            }
        }
    }

    #[test]
    fn applies_after_cursor_to_scan_bounds() {
        let cases = vec![
            ScanCase {
                name: "no cursor keeps unbounded scan",
                setup: ScanSetup::Default,
                direction: Direction::LeftToRight,
                after: None,
                expected: Ok(scan_log(
                    Bound::Unbounded,
                    Bound::Unbounded,
                    Direction::LeftToRight,
                )),
            },
            ScanCase {
                name: "left-to-right cursor tightens left bound",
                setup: range(Bound::Unbounded, Bound::Unbounded),
                direction: Direction::LeftToRight,
                after: Some((2, 20)),
                expected: Ok(scan_log(
                    Bound::Excluded(encode_store_key(2, 20)),
                    Bound::Unbounded,
                    Direction::LeftToRight,
                )),
            },
            ScanCase {
                name: "right-to-left cursor tightens right bound",
                setup: range(Bound::Unbounded, Bound::Unbounded),
                direction: Direction::RightToLeft,
                after: Some((2, 20)),
                expected: Ok(scan_log(
                    Bound::Unbounded,
                    Bound::Excluded(encode_store_key(2, 20)),
                    Direction::RightToLeft,
                )),
            },
            ScanCase {
                name: "cursor inside included range is valid",
                setup: range(
                    Bound::Included(encode_store_key(1, 10)),
                    Bound::Included(encode_store_key(3, 30)),
                ),
                direction: Direction::LeftToRight,
                after: Some((2, 20)),
                expected: Ok(scan_log(
                    Bound::Excluded(encode_store_key(2, 20)),
                    Bound::Included(encode_store_key(3, 30)),
                    Direction::LeftToRight,
                )),
            },
            ScanCase {
                name: "reverse cursor inside included range is valid",
                setup: range(
                    Bound::Included(encode_store_key(1, 10)),
                    Bound::Included(encode_store_key(3, 30)),
                ),
                direction: Direction::RightToLeft,
                after: Some((2, 20)),
                expected: Ok(scan_log(
                    Bound::Included(encode_store_key(1, 10)),
                    Bound::Excluded(encode_store_key(2, 20)),
                    Direction::RightToLeft,
                )),
            },
            ScanCase {
                name: "cursor on included left bound is valid",
                setup: range(
                    Bound::Included(encode_store_key(1, 10)),
                    Bound::Included(encode_store_key(3, 30)),
                ),
                direction: Direction::LeftToRight,
                after: Some((1, 10)),
                expected: Ok(scan_log(
                    Bound::Excluded(encode_store_key(1, 10)),
                    Bound::Included(encode_store_key(3, 30)),
                    Direction::LeftToRight,
                )),
            },
            ScanCase {
                name: "cursor on included right bound is valid",
                setup: range(
                    Bound::Included(encode_store_key(1, 10)),
                    Bound::Included(encode_store_key(3, 30)),
                ),
                direction: Direction::RightToLeft,
                after: Some((3, 30)),
                expected: Ok(scan_log(
                    Bound::Included(encode_store_key(1, 10)),
                    Bound::Excluded(encode_store_key(3, 30)),
                    Direction::RightToLeft,
                )),
            },
            ScanCase {
                name: "cursor below left bound fails",
                setup: range(
                    Bound::Included(encode_store_key(1, 10)),
                    Bound::Included(encode_store_key(3, 30)),
                ),
                direction: Direction::LeftToRight,
                after: Some((0, 10)),
                expected: Err(ErrorKind::CursorOutOfBounds),
            },
            ScanCase {
                name: "cursor above right bound fails",
                setup: range(
                    Bound::Included(encode_store_key(1, 10)),
                    Bound::Included(encode_store_key(3, 30)),
                ),
                direction: Direction::RightToLeft,
                after: Some((4, 10)),
                expected: Err(ErrorKind::CursorOutOfBounds),
            },
            ScanCase {
                name: "cursor on excluded left bound fails",
                setup: range(
                    Bound::Excluded(encode_store_key(1, 10)),
                    Bound::Included(encode_store_key(3, 30)),
                ),
                direction: Direction::LeftToRight,
                after: Some((1, 10)),
                expected: Err(ErrorKind::CursorOutOfBounds),
            },
            ScanCase {
                name: "cursor on excluded right bound fails",
                setup: range(
                    Bound::Included(encode_store_key(1, 10)),
                    Bound::Excluded(encode_store_key(3, 30)),
                ),
                direction: Direction::RightToLeft,
                after: Some((3, 30)),
                expected: Err(ErrorKind::CursorOutOfBounds),
            },
            ScanCase {
                name: "prefix cursor inside bounds is valid",
                setup: ScanSetup::Prefix(2),
                direction: Direction::LeftToRight,
                after: Some((2, 20)),
                expected: Ok(scan_log(
                    Bound::Excluded(encode_store_key(2, 20)),
                    encoded_prefix_range(encode_index_prefix(2)).1,
                    Direction::LeftToRight,
                )),
            },
            ScanCase {
                name: "prefix cursor outside bounds fails",
                setup: ScanSetup::Prefix(2),
                direction: Direction::LeftToRight,
                after: Some((3, 20)),
                expected: Err(ErrorKind::CursorOutOfBounds),
            },
        ];

        for case in cases {
            case.assert();
        }
    }

    #[test]
    fn configures_iter_scan_bounds_from_public_range_builders() {
        let cases = vec![
            ScanCase {
                name: "default scan is unbounded",
                setup: ScanSetup::Default,
                direction: Direction::LeftToRight,
                after: None,
                expected: Ok(scan_log(
                    Bound::Unbounded,
                    Bound::Unbounded,
                    Direction::LeftToRight,
                )),
            },
            ScanCase {
                name: "store-key range configures encoded bounds",
                setup: range(
                    Bound::Included(encode_store_key(1, 10)),
                    Bound::Excluded(encode_store_key(3, 30)),
                ),
                direction: Direction::LeftToRight,
                after: None,
                expected: Ok(scan_log(
                    Bound::Included(encode_store_key(1, 10)),
                    Bound::Excluded(encode_store_key(3, 30)),
                    Direction::LeftToRight,
                )),
            },
            ScanCase {
                name: "prefix configures prefix bounds",
                setup: ScanSetup::Prefix(2),
                direction: Direction::LeftToRight,
                after: None,
                expected: Ok(scan_log(
                    encoded_prefix_range(encode_index_prefix(2)).0,
                    encoded_prefix_range(encode_index_prefix(2)).1,
                    Direction::LeftToRight,
                )),
            },
            ScanCase {
                name: "prefix range excluded upper bound uses encoded prefix",
                setup: ScanSetup::PrefixRange {
                    left: Bound::Included(2),
                    right: Bound::Excluded(4),
                },
                direction: Direction::RightToLeft,
                after: None,
                expected: Ok(scan_log(
                    Bound::Included(encode_index_prefix(2)),
                    Bound::Excluded(encode_index_prefix(4)),
                    Direction::RightToLeft,
                )),
            },
            ScanCase {
                name: "prefix range included upper bound uses prefix end",
                setup: ScanSetup::PrefixRange {
                    left: Bound::Included(2),
                    right: Bound::Included(4),
                },
                direction: Direction::LeftToRight,
                after: None,
                expected: Ok(scan_log(
                    Bound::Included(encode_index_prefix(2)),
                    encoded_prefix_range(encode_index_prefix(4)).1,
                    Direction::LeftToRight,
                )),
            },
            ScanCase {
                name: "prefix range excluded lower bound uses prefix end",
                setup: ScanSetup::PrefixRange {
                    left: Bound::Excluded(2),
                    right: Bound::Excluded(4),
                },
                direction: Direction::LeftToRight,
                after: None,
                expected: Ok(scan_log(
                    encoded_prefix_range(encode_index_prefix(2)).1,
                    Bound::Excluded(encode_index_prefix(4)),
                    Direction::LeftToRight,
                )),
            },
            ScanCase {
                name: "prefix-or-key range supports prefix lower bound",
                setup: ScanSetup::PrefixOrKeyRange {
                    left: Bound::Included(RangeEndpoint::Prefix(2)),
                    right: Bound::Excluded(RangeEndpoint::Key(3, 30)),
                },
                direction: Direction::LeftToRight,
                after: None,
                expected: Ok(scan_log(
                    Bound::Included(encode_index_prefix(2)),
                    Bound::Excluded(encode_store_key(3, 30)),
                    Direction::LeftToRight,
                )),
            },
            ScanCase {
                name: "prefix-or-key range supports key lower bound",
                setup: ScanSetup::PrefixOrKeyRange {
                    left: Bound::Excluded(RangeEndpoint::Key(2, 20)),
                    right: Bound::Included(RangeEndpoint::Prefix(4)),
                },
                direction: Direction::RightToLeft,
                after: None,
                expected: Ok(scan_log(
                    Bound::Excluded(encode_store_key(2, 20)),
                    encoded_prefix_range(encode_index_prefix(4)).1,
                    Direction::RightToLeft,
                )),
            },
            ScanCase {
                name: "prefix-or-key range excluded prefix lower bound uses prefix end",
                setup: ScanSetup::PrefixOrKeyRange {
                    left: Bound::Excluded(RangeEndpoint::Prefix(2)),
                    right: Bound::Excluded(RangeEndpoint::Prefix(4)),
                },
                direction: Direction::LeftToRight,
                after: None,
                expected: Ok(scan_log(
                    encoded_prefix_range(encode_index_prefix(2)).1,
                    Bound::Excluded(encode_index_prefix(4)),
                    Direction::LeftToRight,
                )),
            },
        ];

        for case in cases {
            case.assert();
        }
    }

    fn scan_log(left: Bound<Vec<u8>>, right: Bound<Vec<u8>>, direction: Direction) -> ScanLog {
        ScanLog {
            left,
            right,
            direction,
        }
    }

    fn range(left: Bound<Vec<u8>>, right: Bound<Vec<u8>>) -> ScanSetup {
        ScanSetup::Range { left, right }
    }

    type TestStoreKey = StoreKey<'static, 'static, ByNumber, u32, Record>;

    fn prefix_or_key(endpoint: RangeEndpoint) -> PrefixOrKey<TestStoreKey, u32> {
        match endpoint {
            RangeEndpoint::Prefix(prefix) => PrefixOrKey::Prefix(prefix),
            RangeEndpoint::Key(index, pk) => PrefixOrKey::Key(store_key(index, pk)),
        }
    }

    fn store_key(index: u32, pk: u32) -> StoreKey<'static, 'static, ByNumber, u32, Record> {
        (index, Box::leak(Box::new(pk)))
    }

    fn encode_store_key(index: u32, pk: u32) -> Vec<u8> {
        store_key(index, pk).encode().as_ref().to_vec()
    }

    fn encode_index_prefix(index: u32) -> Vec<u8> {
        (index,).encode().as_ref().to_vec()
    }

    fn decode_store_key(bytes: Vec<u8>) -> StoreKey<'static, 'static, ByNumber, u32, Record> {
        let (index, rest) = u32::decode_part(&bytes);
        let (pk, _) = u32::decode_part(rest);
        store_key(index, pk)
    }
}
