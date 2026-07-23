//! Prefix bounds for ordered composite keys.
//!
//! Prefix scans turn an encoded key prefix into the smallest range that contains
//! every key beginning with those bytes.

use crate::bounds::{IntoScanRange, ScanBound};
use crate::key::Key;
use std::ops::Bound;

/// A typed value that can be used as a scan prefix.
pub trait Prefix: Key {
    /// Encodes this prefix using the same ordered bytes as a full key.
    fn encode_prefix(&self) -> Vec<u8>;
}

impl<P: Prefix> IntoScanRange for P {
    fn start_scan_bound(&self) -> ScanBound {
        let bytes = self.encode_prefix();
        if bytes.is_empty() {
            return Bound::Unbounded;
        }

        Bound::Included(self.encode_prefix())
    }

    fn end_scan_bound(&self) -> ScanBound {
        let bytes = self.encode_prefix();
        if bytes.is_empty() {
            return Bound::Unbounded;
        }

        prefix_end(bytes)
    }
}

pub(crate) fn prefix_end(mut bytes: Vec<u8>) -> Bound<Vec<u8>> {
    for i in (0..bytes.len()).rev() {
        if bytes[i] != 0xff {
            bytes[i] += 1;
            bytes.truncate(i + 1);
            return Bound::Excluded(bytes);
        }
    }

    Bound::Unbounded
}

/// Takes an already encoded prefix and return its range, for testing purposes only.
#[cfg(test)]
pub(crate) fn encoded_prefix_range(prefix: Vec<u8>) -> (Bound<Vec<u8>>, Bound<Vec<u8>>) {
    if prefix.is_empty() {
        return (Bound::Unbounded, Bound::Unbounded);
    }

    let right = prefix_end(prefix.clone());
    (Bound::Included(prefix), right)
}

impl<K> Prefix for K
where
    K: Key,
{
    fn encode_prefix(&self) -> Vec<u8> {
        self.encode().as_ref().to_vec()
    }
}

/// Marker trait proving that `P` is a valid prefix for key `Self`.
pub trait Prefixable<P>
where
    P: Prefix,
{
    type Suffix: Key;

    fn compose(prefix: P, suffix: Self::Suffix) -> Self;
}

impl<A, B> Prefixable<A> for (A, B)
where
    A: Key,
    B: Key,
{
    type Suffix = B;

    fn compose(prefix: A, suffix: Self::Suffix) -> Self {
        (prefix, suffix)
    }
}

impl<A, B, C> Prefixable<A> for (A, B, C)
where
    A: Key,
    B: Key,
    C: Key,
{
    type Suffix = (B, C);

    fn compose(prefix: A, suffix: Self::Suffix) -> Self {
        (prefix, suffix.0, suffix.1)
    }
}

impl<A, B, C> Prefixable<(A, B)> for (A, B, C)
where
    A: Key,
    B: Key,
    C: Key,
{
    type Suffix = C;

    fn compose(prefix: (A, B), suffix: Self::Suffix) -> Self {
        (prefix.0, prefix.1, suffix)
    }
}

impl<A, B, C, D> Prefixable<A> for (A, B, C, D)
where
    A: Key,
    B: Key,
    C: Key,
    D: Key,
{
    type Suffix = (B, C, D);

    fn compose(prefix: A, suffix: Self::Suffix) -> Self {
        (prefix, suffix.0, suffix.1, suffix.2)
    }
}

impl<A, B, C, D> Prefixable<(A, B)> for (A, B, C, D)
where
    A: Key,
    B: Key,
    C: Key,
    D: Key,
{
    type Suffix = (C, D);

    fn compose(prefix: (A, B), suffix: Self::Suffix) -> Self {
        (prefix.0, prefix.1, suffix.0, suffix.1)
    }
}

impl<A, B, C, D> Prefixable<(A, B, C)> for (A, B, C, D)
where
    A: Key,
    B: Key,
    C: Key,
    D: Key,
{
    type Suffix = D;

    fn compose(prefix: (A, B, C), suffix: Self::Suffix) -> Self {
        (prefix.0, prefix.1, prefix.2, suffix)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::KeySize;

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct RawPrefix(Vec<u8>);

    impl Key for RawPrefix {
        const SIZE: KeySize = KeySize::Variable;

        type OwnedKey = RawPrefix;

        type EncodedBytes<'a>
            = Vec<u8>
        where
            Self: 'a;

        fn encode(&self) -> Self::EncodedBytes<'_> {
            self.0.clone()
        }

        fn decode_part(_: &[u8]) -> (Self::OwnedKey, &[u8]) {
            unimplemented!("RawPrefix is only used as an encoded prefix test fixture")
        }
    }

    #[test]
    fn prefix_end_returns_exclusive_successor() {
        let cases = vec![
            (vec![], Bound::Unbounded),
            (vec![0x01], Bound::Excluded(vec![0x02])),
            (vec![0x01, 0x02], Bound::Excluded(vec![0x01, 0x03])),
            (vec![0x01, 0xff], Bound::Excluded(vec![0x02])),
            (vec![0xff], Bound::Unbounded),
            (vec![0xff, 0xff], Bound::Unbounded),
        ];

        for (prefix, expected) in cases {
            assert_eq!(prefix_end(prefix), expected);
        }
    }

    #[test]
    fn raw_prefix_bounds_are_unbounded_for_empty_prefix() {
        let prefix = RawPrefix(vec![]);

        assert_eq!(prefix.start_scan_bound(), Bound::Unbounded);
        assert_eq!(prefix.end_scan_bound(), Bound::Unbounded);
    }

    #[test]
    fn raw_prefix_bounds_cover_prefixed_bytes() {
        let prefix = RawPrefix(vec![0x01, 0x02]);

        assert_eq!(prefix.start_scan_bound(), Bound::Included(vec![0x01, 0x02]));
        assert_eq!(prefix.end_scan_bound(), Bound::Excluded(vec![0x01, 0x03]));
    }

    #[test]
    fn raw_prefix_bounds_without_finite_end_are_upper_unbounded() {
        let prefix = RawPrefix(vec![0xff]);

        assert_eq!(prefix.start_scan_bound(), Bound::Included(vec![0xff]));
        assert_eq!(prefix.end_scan_bound(), Bound::Unbounded);
    }

    #[test]
    fn encoded_prefix_range_uses_already_encoded_bytes() {
        let cases = vec![
            (vec![], (Bound::Unbounded, Bound::Unbounded)),
            (
                vec![0x01, 0x02],
                (
                    Bound::Included(vec![0x01, 0x02]),
                    Bound::Excluded(vec![0x01, 0x03]),
                ),
            ),
            (vec![0xff], (Bound::Included(vec![0xff]), Bound::Unbounded)),
        ];

        for (prefix, expected) in cases {
            assert_eq!(encoded_prefix_range(prefix), expected);
        }
    }
}
