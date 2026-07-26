//! Prefix bounds for ordered composite keys.
//!
//! Prefix scans turn an encoded key prefix into the smallest range that contains
//! every key beginning with those bytes.

use crate::key::Key;
use std::ops::Bound;

/// A typed value that can be used as a scan prefix.
pub trait Prefix: Key {
    /// Encodes this prefix using the same ordered bytes as a full key.
    fn encode_prefix(&self) -> Vec<u8>;
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
///
/// Collette implements this trait for tuple keys whose leftmost elements can be
/// used as a prefix. The associated [`Suffix`](Self::Suffix) is the remaining
/// part of the key that can be ranged over after a prefix has been selected.
///
/// For example, `(Status, CreatedAt)` is [`Prefixable`] by `Status`, and its
/// suffix is `CreatedAt`. This is what makes scans such as
/// `scan.prefix(status).range(created_from..created_to)` type-check.
///
/// You normally do not implement this trait yourself unless you provide a
/// custom composite key type. Application code usually relies on the tuple
/// implementations provided by Collette.
pub trait Prefixable<P>
where
    P: Prefix,
{
    /// Remaining key part after `P` has been fixed.
    type Suffix: Key;

    /// Rebuilds the complete key from a prefix and suffix.
    ///
    /// Scan builders use this to turn a suffix range into concrete key bounds
    /// inside the selected prefix.
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

    #[test]
    fn key_values_encode_as_prefixes() {
        assert_eq!(42u32.encode_prefix(), 42u32.encode().as_ref());
        assert_eq!((1u8, 2u16).encode_prefix(), (1u8, 2u16).encode().as_ref());
    }

    #[test]
    fn two_part_keys_compose_from_one_part_prefix() {
        type Key = (u8, u16);

        assert_eq!(<Key as Prefixable<u8>>::compose(1, 2), (1, 2));
    }

    #[test]
    fn three_part_keys_compose_from_supported_prefixes() {
        type Key = (u8, u16, u32);

        assert_eq!(<Key as Prefixable<u8>>::compose(1, (2, 3)), (1, 2, 3));
        assert_eq!(
            <Key as Prefixable<(u8, u16)>>::compose((1, 2), 3),
            (1, 2, 3)
        );
    }

    #[test]
    fn four_part_keys_compose_from_supported_prefixes() {
        type Key = (u8, u16, u32, u64);

        assert_eq!(<Key as Prefixable<u8>>::compose(1, (2, 3, 4)), (1, 2, 3, 4));
        assert_eq!(
            <Key as Prefixable<(u8, u16)>>::compose((1, 2), (3, 4)),
            (1, 2, 3, 4)
        );
        assert_eq!(
            <Key as Prefixable<(u8, u16, u32)>>::compose((1, 2, 3), 4),
            (1, 2, 3, 4)
        );
    }
}
