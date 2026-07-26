use crate::key::Key;
use crate::prefix::{prefix_end, Prefix, Prefixable};
use std::ops::{Bound, RangeBounds};

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

pub struct PrefixEncoder {}

impl<K: Key> BoundsEncoder<K> for PrefixEncoder {
    fn encode_start_bound(bound: Bound<K>) -> ScanBound {
        match bound {
            Bound::Included(k) => Bound::Included(k.encode().as_ref().to_vec()),
            Bound::Excluded(k) => prefix_end(k.encode().as_ref().to_vec()),
            Bound::Unbounded => Bound::Unbounded,
        }
    }

    fn encode_end_bound(bound: Bound<K>) -> ScanBound {
        match bound {
            Bound::Included(k) => prefix_end(k.encode().as_ref().to_vec()),
            Bound::Excluded(k) => Bound::Excluded(k.encode().as_ref().to_vec()),
            Bound::Unbounded => Bound::Unbounded,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(key: impl Key) -> Vec<u8> {
        key.encode().as_ref().to_vec()
    }

    #[test]
    fn exact_encoder_preserves_bound_inclusivity() {
        let bounds = ExactEncoder::encode_range((
            Bound::Included((1u32, 10u16)),
            Bound::Excluded((2u32, 20u16)),
        ));

        assert_eq!(
            bounds,
            (
                Bound::Included(encode((1u32, 10u16))),
                Bound::Excluded(encode((2u32, 20u16))),
            )
        );
    }

    #[test]
    fn prefix_encoder_excludes_start_logical_key_group() {
        let bounds =
            PrefixEncoder::encode_range((Bound::Excluded((1u32, 10u16)), Bound::Unbounded));

        assert_eq!(
            bounds,
            (prefix_end(encode((1u32, 10u16))), Bound::Unbounded)
        );
    }

    #[test]
    fn prefix_encoder_includes_end_logical_key_group() {
        let bounds = PrefixEncoder::encode_range(..=(1u32, 10u16));

        assert_eq!(
            bounds,
            (Bound::Unbounded, prefix_end(encode((1u32, 10u16))))
        );
    }

    #[test]
    fn exact_prefixed_range_composes_single_prefix_and_suffix_bounds() {
        let range = <ExactEncoder as BoundsEncoder<(u32, u16)>>::encode_prefixed_range(
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
    fn exact_prefixed_range_clamps_unbounded_suffix_bounds() {
        let range = <ExactEncoder as BoundsEncoder<(u32, u16)>>::encode_prefixed_range(7u32, ..);

        assert_eq!(
            range,
            (
                Bound::Included(encode(7u32)),
                crate::prefix::prefix_end(encode(7u32))
            )
        );
    }

    #[test]
    fn prefix_prefixed_range_includes_end_logical_key_group() {
        let range =
            <PrefixEncoder as BoundsEncoder<(u32, u16)>>::encode_prefixed_range(7u32, ..=20u16);

        assert_eq!(
            range,
            (
                Bound::Included(encode(7u32)),
                prefix_end(encode((7u32, 20u16)))
            )
        );
    }

    #[test]
    fn prefix_prefixed_range_excludes_start_logical_key_group() {
        let range = <PrefixEncoder as BoundsEncoder<(u32, u16)>>::encode_prefixed_range(
            7u32,
            (Bound::Excluded(10u16), Bound::Unbounded),
        );

        assert_eq!(
            range,
            (
                prefix_end(encode((7u32, 10u16))),
                crate::prefix::prefix_end(encode(7u32))
            )
        );
    }
}
