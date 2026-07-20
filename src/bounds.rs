use crate::key::Key;
use crate::prefix::{Prefix, PrefixOrKey, Prefixable};
use std::ops::{Bound, Range};

pub(crate) type ScanBound = Bound<Vec<u8>>;
pub(crate) type ScanRange = (ScanBound, ScanBound);

pub(crate) trait IntoScanBounds {
    fn start_bound(&self) -> ScanBound;

    fn end_bound(&self) -> ScanBound;

    fn range(&self) -> ScanRange {
        (self.start_bound(), self.end_bound())
    }
}

impl<K: Key> IntoScanBounds for Range<Bound<K>> {
    fn start_bound(&self) -> Bound<Vec<u8>> {
        self.start.clone().map(|k| k.encode().as_ref().to_vec())
    }

    fn end_bound(&self) -> ScanBound {
        self.end.clone().map(|k| k.encode().as_ref().to_vec())
    }
}

impl<P: Prefix> IntoScanBounds for Bound<P> {
    fn start_bound(&self) -> Bound<Vec<u8>> {
        match self {
            Bound::Included(prefix) => prefix.start_bound(),
            Bound::Excluded(prefix) => prefix.end_bound(),
            Bound::Unbounded => Bound::Unbounded,
        }
    }

    fn end_bound(&self) -> Bound<Vec<u8>> {
        match self {
            Bound::Included(prefix) => prefix.end_bound(),
            Bound::Excluded(prefix) => match prefix.start_bound() {
                Bound::Included(bytes) | Bound::Excluded(bytes) => Bound::Excluded(bytes),
                Bound::Unbounded => Bound::Unbounded,
            },
            Bound::Unbounded => Bound::Unbounded,
        }
    }
}

impl<K: Key + Prefixable<P>, P: Prefix> IntoScanBounds for Bound<PrefixOrKey<K, P>> {
    fn start_bound(&self) -> Bound<Vec<u8>> {
        match self {
            Bound::Included(PrefixOrKey::Prefix(prefix)) => prefix.start_bound(),
            Bound::Excluded(PrefixOrKey::Prefix(prefix)) => prefix.end_bound(),
            Bound::Included(PrefixOrKey::Key(key)) => {
                Bound::Included(key.encode().as_ref().to_vec())
            }
            Bound::Excluded(PrefixOrKey::Key(key)) => {
                Bound::Excluded(key.encode().as_ref().to_vec())
            }
            Bound::Unbounded => Bound::Unbounded,
        }
    }

    fn end_bound(&self) -> Bound<Vec<u8>> {
        match self {
            Bound::Included(PrefixOrKey::Prefix(prefix)) => prefix.end_bound(),
            Bound::Excluded(PrefixOrKey::Prefix(prefix)) => match prefix.start_bound() {
                Bound::Included(bytes) | Bound::Excluded(bytes) => Bound::Excluded(bytes),
                Bound::Unbounded => Bound::Unbounded,
            },
            Bound::Included(PrefixOrKey::Key(key)) => {
                Bound::Included(key.encode().as_ref().to_vec())
            }
            Bound::Excluded(PrefixOrKey::Key(key)) => {
                Bound::Excluded(key.encode().as_ref().to_vec())
            }
            Bound::Unbounded => Bound::Unbounded,
        }
    }
}

pub (crate) fn prefix_range<K, P, S>(prefix: P, range: Range<Bound<S>>) -> Range<Bound<K>>
where
    K: Key + Prefixable<P, Suffix = S>,
    P: Prefix,
    S: Key,
{
    Range {
        start: range.start.map(|s| K::compose(prefix.clone(), s)),
        end: range.end.map(|s| K::compose(prefix, s)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefix::encoded_prefix_range;

    #[test]
    fn prefix_bounds_convert_to_scan_bounds() {
        let prefix = 2u32;
        let encoded = prefix.encode_prefix();
        let range = encoded_prefix_range(encoded.clone());
        let cases = vec![
            (
                "included prefix starts at prefix and ends after prefix",
                Bound::Included(prefix),
                range.0.clone(),
                range.1.clone(),
            ),
            (
                "excluded prefix starts after prefix and ends before prefix",
                Bound::Excluded(prefix),
                range.1,
                Bound::Excluded(encoded),
            ),
            (
                "unbounded prefix remains unbounded",
                Bound::Unbounded,
                Bound::Unbounded,
                Bound::Unbounded,
            ),
        ];

        for (name, bound, expected_start, expected_end) in cases {
            assert_eq!(bound.start_bound(), expected_start, "{name}");
            assert_eq!(bound.end_bound(), expected_end, "{name}");
        }
    }

    #[test]
    fn prefix_or_key_bounds_convert_to_scan_bounds() {
        let prefix = 2u32;
        let encoded_prefix = prefix.encode_prefix();
        let prefix_range = encoded_prefix_range(encoded_prefix.clone());
        let key = (2u32, 20u32);
        let encoded_key = key.encode().as_ref().to_vec();
        let cases = vec![
            (
                "included prefix starts at prefix and ends after prefix",
                Bound::Included(PrefixOrKey::Prefix(prefix)),
                prefix_range.0.clone(),
                prefix_range.1.clone(),
            ),
            (
                "excluded prefix starts after prefix and ends before prefix",
                Bound::Excluded(PrefixOrKey::Prefix(prefix)),
                prefix_range.1,
                Bound::Excluded(encoded_prefix),
            ),
            (
                "included key uses exact included key bounds",
                Bound::Included(PrefixOrKey::Key(key)),
                Bound::Included(encoded_key.clone()),
                Bound::Included(encoded_key.clone()),
            ),
            (
                "excluded key uses exact excluded key bounds",
                Bound::Excluded(PrefixOrKey::Key(key)),
                Bound::Excluded(encoded_key.clone()),
                Bound::Excluded(encoded_key),
            ),
            (
                "unbounded prefix or key remains unbounded",
                Bound::Unbounded,
                Bound::Unbounded,
                Bound::Unbounded,
            ),
        ];

        for (name, bound, expected_start, expected_end) in cases {
            assert_eq!(bound.start_bound(), expected_start, "{name}");
            assert_eq!(bound.end_bound(), expected_end, "{name}");
        }
    }
}
