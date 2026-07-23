use crate::key::Key;
use crate::prefix::{Prefix, Prefixable};
use std::ops::{Bound, Range, RangeBounds};

pub(crate) type ScanBound = Bound<Vec<u8>>;
pub(crate) type ScanRange = (ScanBound, ScanBound);

pub(crate) trait IntoScanBounds {
    fn start_bound(&self) -> ScanBound;

    fn end_bound(&self) -> ScanBound;

    fn range(&self) -> ScanRange {
        (self.start_bound(), self.end_bound())
    }
}

impl<K: Key> IntoScanBounds for (Bound<K>, Bound<K>)
{
    fn start_bound(&self) -> Bound<Vec<u8>> {
        self.0.clone().map(|k| k.encode().as_ref().to_vec())
    }

    fn end_bound(&self) -> ScanBound {
        self.1.clone().map(|k| k.encode().as_ref().to_vec())
    }
}

impl<K: Key> IntoScanBounds for Range<Bound<K>>
{
    fn start_bound(&self) -> Bound<Vec<u8>> {
        self.start.clone().map(|k| k.encode().as_ref().to_vec())
    }

    fn end_bound(&self) -> ScanBound {
        self.end.clone().map(|k| k.encode().as_ref().to_vec())
    }
}

pub (crate) fn prefixed_range<K, P, S>(prefix: P, range: impl RangeBounds<S>) -> (Bound<K>, Bound<K>)
where
    K: Key + Prefixable<P, Suffix = S>,
    P: Prefix,
    S: Key
{
    (
        range.start_bound().cloned().map(|s| K::compose(prefix.clone(), s)),
        range.end_bound().cloned().map(|s| K::compose(prefix, s)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
}
