//! Typed scan builders for secondary indexes.
//!
//! Scans are lazy: they collect bounds, direction, and cursor information until
//! [`IndexScan::iter`] opens the backend stores and returns an iterator.

use crate::bounds::{prefixed_range, IntoScanBounds, ScanBound, ScanRange};
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

pub trait PrefixableScan<'a, K: Key + Prefixable<P>, P: Prefix>: Scan
where
    Self: 'a,
    <Self as Scan>::Key<'a>: Key + Prefixable<P>,
{
    fn prefix(self, prefix: P) -> PrefixedScan<'a, Self, P>;
}

impl<'a, ReadHandle, Record, Idx, P> PrefixableScan<'a, <Self as Scan>::Key<'a>, P> for IndexFullScan<'a, ReadHandle, Record, Idx>
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

impl<'a, S, P> PrefixedScan<'a, S, P>
where
    S: Scan,
    P: Prefix,
    S::Key<'a>: Prefixable<P>,
{
    pub fn range<R>(self, range: R) -> RangeScan<'a, Self, (Bound<S::Key<'a>>, Bound<S::Key<'a>>)>
    where
        R: RangeBounds<<S::Key<'a> as Prefixable<P>>::Suffix>,
    {
        RangeScan {
            range: prefixed_range(self.prefix.clone(), range),
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

impl<'a, S, R> RangeScan<'a, S, R>
where
    S: Scan,
    R: RangeBounds<S::Key<'a>>,
{
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

impl<'a, S> DirectedScan<'a, S>
where
    S: Scan,
{
    pub fn after(self, cursor: Vec<u8>) -> AfterScan<'a, Self> {
        AfterScan {
            cursor,
            inner: self,
            _marker: Default::default(),
        }
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
        build: impl FnOnce(IndexFullScan<'static, MockReadHandle, Record, ByNumber>) -> S,
        expected: Result<ScanLog, ErrorKind>,
    ) where
        S: Scan<Executor = IndexExecutor<MockReadHandle, Record, ByNumber>>,
    {
        let db = MockDb::new();
        let log = db.log();
        let read = db.read("records").unwrap();
        let scan = build(IndexFullScan::<_, Record, ByNumber>::new(read, "records"));

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
}
