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
}
