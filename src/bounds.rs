use crate::key::Key;
use crate::prefix::{Prefix, Prefixable};
use std::ops::{Bound, Range, RangeBounds};

pub(crate) type ScanBound = Bound<Vec<u8>>;
pub(crate) type ScanRange = (ScanBound, ScanBound);

pub(crate) trait IntoScanRange {
    fn start_scan_bound(&self) -> ScanBound;

    fn end_scan_bound(&self) -> ScanBound;

    fn range(&self) -> ScanRange {
        (self.start_scan_bound(), self.end_scan_bound())
    }
}

impl<K: Key> IntoScanRange for (Bound<K>, Bound<K>) {
    fn start_scan_bound(&self) -> ScanBound {
        self.0.clone().map(|k| k.encode().as_ref().to_vec())
    }

    fn end_scan_bound(&self) -> ScanBound {
        self.1.clone().map(|k| k.encode().as_ref().to_vec())
    }
}

impl<K: Key> IntoScanRange for Range<Bound<K>> {
    fn start_scan_bound(&self) -> ScanBound {
        self.start.clone().map(|k| k.encode().as_ref().to_vec())
    }

    fn end_scan_bound(&self) -> ScanBound {
        self.end.clone().map(|k| k.encode().as_ref().to_vec())
    }
}

/// Composes suffix bounds with an already selected prefix.
///
/// This helper keeps prefix scan composition typed. Given a complete key `K`, a
/// valid prefix `P`, and a range over `K`'s suffix `S`, it returns bounds over
/// the complete key.
///
/// It does not encode the resulting bounds. Encoding happens later through
/// [`IntoScanRange`], when a scan is compiled.
///
/// # Examples
///
/// ```rust,ignore
/// # use std::ops::Bound;
/// # use collette::bounds::prefixed_range;
/// let bounds: (Bound<(u8, u64)>, Bound<(u8, u64)>) =
///     prefixed_range(1u8, 10u64..20u64);
///
/// assert_eq!(bounds, (Bound::Included((1, 10)), Bound::Excluded((1, 20))));
/// ```
pub(crate) fn prefixed_range<K, P, S>(prefix: P, range: impl RangeBounds<S>) -> (Bound<K>, Bound<K>)
where
    K: Key + Prefixable<P, Suffix = S>,
    P: Prefix,
    S: Key,
{
    (
        range
            .start_bound()
            .cloned()
            .map(|s| K::compose(prefix.clone(), s)),
        range.end_bound().cloned().map(|s| K::compose(prefix, s)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bound_pair_converts_keys_to_scan_bounds() {
        let bounds = (
            Bound::Included((1u32, 10u16)),
            Bound::Excluded((2u32, 20u16)),
        );

        assert_eq!(
            bounds.range(),
            (
                Bound::Included(encode((1u32, 10u16))),
                Bound::Excluded(encode((2u32, 20u16))),
            )
        );
    }

    #[test]
    fn bound_range_converts_keys_to_scan_bounds() {
        let range = Bound::Excluded(1u32)..Bound::Included(3u32);

        assert_eq!(
            range.range(),
            (Bound::Excluded(encode(1u32)), Bound::Included(encode(3u32)),)
        );
    }

    #[test]
    fn prefixed_range_composes_single_prefix_and_suffix_bounds() {
        let range: (Bound<(u32, u16)>, Bound<(u32, u16)>) =
            prefixed_range(7u32, (Bound::Excluded(10u16), Bound::Included(20u16)));

        assert_eq!(
            range,
            (
                Bound::Excluded((7u32, 10u16)),
                Bound::Included((7u32, 20u16))
            )
        );
    }

    #[test]
    fn prefixed_range_preserves_unbounded_suffix_bounds() {
        let range: (Bound<(u32, u16)>, Bound<(u32, u16)>) =
            prefixed_range::<(u32, u16), u32, u16>(7u32, ..);

        assert_eq!(range, (Bound::Unbounded, Bound::Unbounded));
    }

    #[test]
    fn prefixed_range_composes_tuple_prefix_and_suffix_bounds() {
        let range: (Bound<(u32, u16, u8)>, Bound<(u32, u16, u8)>) =
            prefixed_range((7u32, 8u16), 9u8..10u8);

        assert_eq!(
            range,
            (
                Bound::Included((7u32, 8u16, 9u8)),
                Bound::Excluded((7u32, 8u16, 10u8))
            )
        );
    }

    fn encode(key: impl Key) -> Vec<u8> {
        key.encode().as_ref().to_vec()
    }
}
