use crate::key::Key;
use crate::prefix::{prefix_end, Prefix, Prefixable};
use std::ops::{Bound, Range, RangeBounds};

pub(crate) type ScanBound = Bound<Vec<u8>>;
pub(crate) type ScanRange = (ScanBound, ScanBound);

pub trait BoundsEncoder<K: Key> {
    fn encode_start_bound(bound: Bound<K>) -> ScanBound;

    fn encode_end_bound(bound: Bound<K>) -> ScanBound;

    fn encode_range(range: impl RangeBounds<K>) -> ScanRange {
        (
            Self::encode_start_bound(range.start_bound().cloned()),
            Self::encode_end_bound(range.end_bound().cloned()),
        )
    }

    /// Composes suffix bounds with an already selected prefix.
    ///
    /// This helper compiles prefix scan composition. Given a complete key `K`, a
    /// valid prefix `P`, and a range over `K`'s suffix `S`, it returns encoded scan
    /// bounds clamped to the selected prefix.
    fn encode_prefixed_range<P, S>(prefix: P, range: impl RangeBounds<S>) -> ScanRange
    where
        K: Prefixable<P, Suffix = S>,
        P: Prefix,
        S: Key,
    {
        let (prefix_start, prefix_end) = Self::encode_prefix(&prefix);
        (
            match range.start_bound().cloned() {
                Bound::Included(suffix) => {
                    Self::encode_start_bound(Bound::Included(K::compose(prefix.clone(), suffix)))
                }
                Bound::Excluded(suffix) => {
                    Self::encode_start_bound(Bound::Excluded(K::compose(prefix.clone(), suffix)))
                }
                Bound::Unbounded => prefix_start,
            },
            match range.end_bound().cloned() {
                Bound::Included(suffix) => {
                    Self::encode_end_bound(Bound::Included(K::compose(prefix, suffix)))
                }
                Bound::Excluded(suffix) => {
                    Self::encode_end_bound(Bound::Excluded(K::compose(prefix, suffix)))
                }
                Bound::Unbounded => prefix_end,
            },
        )
    }

    fn encode_prefix<P>(prefix: &P) -> ScanRange
    where
        K: Prefixable<P>,
        P: Prefix,
    {
        let bound = prefix.encode_prefix();
        (Bound::Included(bound.clone()), prefix_end(bound))
    }
}

pub struct ExactEncoder {}

impl<K: Key> BoundsEncoder<K> for ExactEncoder {
    fn encode_start_bound(bound: Bound<K>) -> ScanBound {
        bound.map(|k| k.encode().as_ref().to_vec())
    }

    fn encode_end_bound(bound: Bound<K>) -> ScanBound {
        bound.map(|k| k.encode().as_ref().to_vec())
    }
}
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
/// This helper compiles prefix scan composition. Given a complete key `K`, a
/// valid prefix `P`, and a range over `K`'s suffix `S`, it returns encoded scan
/// bounds clamped to the selected prefix.
/// ```
pub(crate) fn prefixed_range<K, P, S>(prefix: P, range: impl RangeBounds<S>) -> ScanRange
where
    K: Key + Prefixable<P, Suffix = S>,
    P: Prefix,
    S: Key,
{
    (
        prefixed_start_bound::<K, _, _>(prefix.clone(), range.start_bound()),
        prefixed_end_bound::<K, _, _>(prefix, range.end_bound()),
    )
}

fn prefixed_start_bound<K, P, S>(prefix: P, bound: Bound<&S>) -> ScanBound
where
    K: Key + Prefixable<P, Suffix = S>,
    P: Prefix,
    S: Key,
{
    match bound {
        Bound::Included(suffix) => Bound::Included(encode(K::compose(prefix, suffix.clone()))),
        Bound::Excluded(suffix) => Bound::Excluded(encode(K::compose(prefix, suffix.clone()))),
        Bound::Unbounded => prefix.start_scan_bound(),
    }
}

fn prefixed_end_bound<K, P, S>(prefix: P, bound: Bound<&S>) -> ScanBound
where
    K: Key + Prefixable<P, Suffix = S>,
    P: Prefix,
    S: Key,
{
    match bound {
        Bound::Included(suffix) => Bound::Included(encode(K::compose(prefix, suffix.clone()))),
        Bound::Excluded(suffix) => Bound::Excluded(encode(K::compose(prefix, suffix.clone()))),
        Bound::Unbounded => prefix.end_scan_bound(),
    }
}

fn encode(key: impl Key) -> Vec<u8> {
    key.encode().as_ref().to_vec()
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
        let range = prefixed_range::<(u32, u16), _, _>(
            7u32,
            (Bound::Excluded(10u16), Bound::Included(20u16)),
        );

        assert_eq!(
            range,
            (
                Bound::Excluded(encode((7u32, 10u16))),
                Bound::Included(encode((7u32, 20u16)))
            )
        );
    }

    #[test]
    fn prefixed_range_preserves_unbounded_suffix_bounds() {
        let range = prefixed_range::<(u32, u16), u32, u16>(7u32, ..);

        assert_eq!(
            range,
            (
                Bound::Included(encode(7u32)),
                crate::prefix::prefix_end(encode(7u32))
            )
        );
    }

    #[test]
    fn prefixed_range_clamps_unbounded_suffix_start() {
        let range = prefixed_range::<(u32, u16), u32, u16>(7u32, ..20u16);

        assert_eq!(
            range,
            (
                Bound::Included(encode(7u32)),
                Bound::Excluded(encode((7u32, 20u16)))
            )
        );
    }

    #[test]
    fn prefixed_range_clamps_unbounded_suffix_end() {
        let range = prefixed_range::<(u32, u16), u32, u16>(7u32, 10u16..);

        assert_eq!(
            range,
            (
                Bound::Included(encode((7u32, 10u16))),
                crate::prefix::prefix_end(encode(7u32))
            )
        );
    }

    #[test]
    fn prefixed_range_composes_tuple_prefix_and_suffix_bounds() {
        let range = prefixed_range::<(u32, u16, u8), _, _>((7u32, 8u16), 9u8..10u8);

        assert_eq!(
            range,
            (
                Bound::Included(encode((7u32, 8u16, 9u8))),
                Bound::Excluded(encode((7u32, 8u16, 10u8)))
            )
        );
    }
}
