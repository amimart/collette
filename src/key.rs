//! Ordered key encodings used by primary keys, index keys, and scan bounds.
//!
//! Implementations must encode values so that byte-wise lexicographic ordering
//! matches the logical ordering of the Rust value. Collette provides
//! implementations for common scalar keys, byte keys, string keys, and tuples.

use crate::inline_vec::IVec;
use crate::{impl_signed_integer_key, impl_unsigned_integer_key};
use std::fmt::Debug;

/// A value that can be encoded as an ordered key for Collette stores and indexes.
///
/// The encoded representation must preserve the logical ordering of the value.
///
/// Encoded keys are used for:
///
/// - primary keys;
/// - secondary indexes;
/// - range scans;
/// - prefix scans;
/// - cursor pagination.
///
/// # Ordering
///
/// Ordered KV stores compare keys lexicographically. Numeric values should
/// therefore generally use big-endian encoding.
///
/// # Variable-size keys
///
/// Variable-size values such as `str`, `String`, `[u8]` or `Vec<u8>` must use
/// escaping/framing so that composite keys remain unambiguous.
///
/// Implementations for variable-size keys should use the helper functions
/// provided by Collette to encode and decode variable-size key bytes:
/// - `encode_unsized_key_bytes`;
/// - `decode_unsized_key_bytes`;
///
/// # Composite keys
///
/// Composite keys are represented using tuples. Their encoding is obtained by
/// concatenating the encoding of each component.
///
/// # Compatibility
///
/// Changing a `Key` implementation changes the physical storage layout and
/// should be treated as a migration.
///
/// # Examples
///
/// Built-in integer keys preserve numeric ordering in their encoded bytes:
///
/// ```
/// use collette::Key;
///
/// assert!(1u64.encode().as_ref() < 2u64.encode().as_ref());
/// assert_eq!(u64::decode(&42u64.encode()), 42);
/// ```
pub trait Key: Clone + Debug + Eq {
    /// The encoded size of the key.
    ///
    /// Fixed-size keys allow Collette to preallocate buffers efficiently.
    const SIZE: KeySize;

    /// Owned form produced when decoding this key.
    ///
    /// Borrowed key types such as `&str` decode to an owned value such as
    /// [`String`].
    type OwnedKey: Key + Sized;

    /// Byte buffer returned by [`encode`](Self::encode).
    ///
    /// Fixed-size borrowed keys can return references, while variable-size
    /// composite keys commonly return an inline buffer.
    type EncodedBytes<'a>: AsRef<[u8]> + 'a
    where
        Self: 'a;

    /// Returns the encoded key.
    ///
    /// Implementations should rely if possible on the underlying bytes of the key (e.g. for
    /// fixed-size keys) to avoid unnecessary allocations.
    ///
    /// Variable-size keys should use Collette encoding helper (i.e. `encode_unsized_key_bytes`) to
    /// ensure their encoded representation remains safe for composite keys and prefix scans.
    fn encode(&self) -> Self::EncodedBytes<'_>;

    /// Decodes a key from its encoded representation and returns the unread bytes.
    ///
    /// Panics if the input is malformed or incomplete.
    ///
    /// Variable-size keys should use Collette decoding helper (i.e. `decode_unsized_key_bytes`) to
    /// decode escaped and framed key bytes correctly.
    fn decode_part(bytes: &[u8]) -> (Self::OwnedKey, &[u8]);

    /// Decodes a key from its encoded representation.
    ///
    /// Panics if the input is malformed, incomplete, or more than complete.
    ///
    /// Variable-size keys should use Collette decoding helper (i.e. `decode_unsized_key_bytes`) to
    /// decode escaped and framed key bytes correctly.
    fn decode(bytes: &[u8]) -> Self::OwnedKey {
        let (key, rest) = Self::decode_part(bytes);
        if !rest.is_empty() {
            panic!("bytes contains more data than expected")
        }
        key
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Whether a key encoding always has the same byte length.
pub enum KeySize {
    /// The key always encodes to the given number of bytes.
    Fixed(usize),
    /// The key encoding length depends on the value.
    Variable,
}

/// Encodes variable-size key bytes using Collette escaping and framing rules.
///
/// The `0x00` byte is reserved internally:
///
/// - `0x00 0xff` represents an escaped `0x00` byte;
/// - `0x00 0x00` marks the end of the encoded value.
///
/// This encoding ensures that variable-size keys remain safe to concatenate
/// inside composite keys while preserving lexicographic ordering.
pub fn encode_unsized_key_bytes(bytes: &[u8], out: &mut IVec) {
    for &b in bytes {
        match b {
            0x00 => out.extend_from_slice(&[0x00, 0xff]),
            b => out.push(b),
        }
    }
    out.extend_from_slice(&[0x00, 0x00]);
}

/// Decodes variable-size key bytes encoded with
/// `encode_unsized_key_bytes`.
///
/// Panics if the encoded bytes contain invalid escape sequences or are missing
/// the terminating marker.
pub fn decode_unsized_key_bytes(bytes: &[u8]) -> (Vec<u8>, &[u8]) {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            0x00 => {
                let next = bytes.get(i + 1).unwrap();
                match next {
                    0x00 => {
                        // terminator
                        return (out, bytes.get(i + 2..).unwrap_or(&[]));
                    }
                    0xff => {
                        // escaped 0x00
                        out.push(0x00);
                        i += 2;
                    }
                    other => {
                        panic!("invalid escape sequence in encoded key bytes: expected 0x00 0x00 or 0x00 0xff, got 0x00 {other:02x}");
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    panic!("unterminated encoded key bytes: expected terminator 0x00 0x00 not found");
}

impl<K: Key> Key for &K {
    const SIZE: KeySize = K::SIZE;

    type OwnedKey = K::OwnedKey;

    type EncodedBytes<'a>
        = K::EncodedBytes<'a>
    where
        Self: 'a;

    fn encode(&self) -> Self::EncodedBytes<'_> {
        (*self).encode()
    }

    fn decode_part(bytes: &[u8]) -> (Self::OwnedKey, &[u8]) {
        K::decode_part(bytes)
    }
}

impl<K: Key> Key for (K,) {
    const SIZE: KeySize = K::SIZE;

    type OwnedKey = (K::OwnedKey,);

    type EncodedBytes<'a>
        = K::EncodedBytes<'a>
    where
        Self: 'a;

    fn encode(&self) -> Self::EncodedBytes<'_> {
        self.0.encode()
    }

    fn decode_part(bytes: &[u8]) -> (Self::OwnedKey, &[u8]) {
        let (k, r) = K::decode_part(bytes);
        ((k,), r)
    }
}

impl_unsigned_integer_key!(u8);
impl_unsigned_integer_key!(u16);
impl_unsigned_integer_key!(u32);
impl_unsigned_integer_key!(u64);
impl_unsigned_integer_key!(u128);
impl_signed_integer_key!(i8 => u8);
impl_signed_integer_key!(i16 => u16);
impl_signed_integer_key!(i32 => u32);
impl_signed_integer_key!(i64 => u64);
impl_signed_integer_key!(i128 => u128);

impl Key for bool {
    const SIZE: KeySize = KeySize::Fixed(1);

    type OwnedKey = Self;

    type EncodedBytes<'a>
        = [u8; 1]
    where
        Self: 'a;

    fn encode(&self) -> Self::EncodedBytes<'_> {
        match self {
            true => [1],
            false => [0],
        }
    }

    fn decode_part(bytes: &[u8]) -> (Self::OwnedKey, &[u8]) {
        match bytes {
            [0, r @ ..] => (false, r),
            [1, r @ ..] => (true, r),
            _ => panic!("invalid boolean bytes"),
        }
    }
}

/// As Rust Strings are guaranteed UTF-8 we don't need escaping, so we just use `0xff` (i.e.
/// forbidden in utf-8) as end byte.
impl Key for String {
    const SIZE: KeySize = KeySize::Variable;

    type OwnedKey = Self;

    type EncodedBytes<'a>
        = IVec
    where
        Self: 'a;

    fn encode(&self) -> Self::EncodedBytes<'_> {
        self.as_str().encode()
    }

    fn decode_part(bytes: &[u8]) -> (Self::OwnedKey, &[u8]) {
        <&str as Key>::decode_part(bytes)
    }
}

/// As Rust Strings are guaranteed UTF-8 we don't need escaping, so we just use `0xff` (i.e.
/// forbidden in utf-8) as end byte.
impl Key for &str {
    const SIZE: KeySize = KeySize::Variable;

    type OwnedKey = String;

    type EncodedBytes<'a>
        = IVec
    where
        Self: 'a;

    fn encode(&self) -> Self::EncodedBytes<'_> {
        let mut out = IVec::with_capacity(self.len() + 1);
        out.extend_from_slice(self.as_bytes());
        out.push(0xff);
        out
    }

    fn decode_part(bytes: &[u8]) -> (Self::OwnedKey, &[u8]) {
        let end_str_pos = bytes.iter().position(|&b| b == 0xff).unwrap();
        let (strbytes, tail) = bytes.split_at(end_str_pos);

        (
            str::from_utf8(strbytes).unwrap().to_string(),
            match tail {
                [_, r @ ..] => r,
                _ => panic!("invalid encoded string bytes"),
            },
        )
    }
}

impl<const S: usize> Key for [u8; S] {
    const SIZE: KeySize = KeySize::Fixed(S);

    type OwnedKey = Self;

    type EncodedBytes<'a>
        = &'a Self
    where
        Self: 'a;

    fn encode(&self) -> Self::EncodedBytes<'_> {
        self
    }

    fn decode_part(bytes: &[u8]) -> (Self::OwnedKey, &[u8]) {
        let (kbytes, r) = bytes.split_at(S);
        (kbytes.try_into().unwrap(), r)
    }
}

impl Key for Vec<u8> {
    const SIZE: KeySize = KeySize::Variable;

    type OwnedKey = Self;

    type EncodedBytes<'a>
        = IVec
    where
        Self: 'a;

    fn encode(&self) -> Self::EncodedBytes<'_> {
        let mut out = IVec::with_capacity(self.len() + 2);
        encode_unsized_key_bytes(self, &mut out);
        out
    }

    fn decode_part(bytes: &[u8]) -> (Self::OwnedKey, &[u8]) {
        decode_unsized_key_bytes(bytes)
    }
}

impl<A, B> Key for (A, B)
where
    A: Key,
    B: Key,
{
    const SIZE: KeySize = match (A::SIZE, B::SIZE) {
        (KeySize::Fixed(s1), KeySize::Fixed(s2)) => KeySize::Fixed(s1 + s2),
        _ => KeySize::Variable,
    };

    type OwnedKey = (A::OwnedKey, B::OwnedKey);

    type EncodedBytes<'a>
        = IVec
    where
        Self: 'a;

    fn encode(&self) -> Self::EncodedBytes<'_> {
        match Self::SIZE {
            KeySize::Fixed(s) => {
                let mut out = IVec::with_capacity(s);
                out.extend_from_slice(self.0.encode().as_ref());
                out.extend_from_slice(self.1.encode().as_ref());
                out
            }
            KeySize::Variable => {
                let mut out = IVec::from(self.0.encode().as_ref());
                out.extend_from_slice(self.1.encode().as_ref());
                out
            }
        }
    }

    fn decode_part(bytes: &[u8]) -> (Self::OwnedKey, &[u8]) {
        let (a, r) = A::decode_part(bytes);
        let (b, r) = B::decode_part(r);
        ((a, b), r)
    }
}

impl<A, B, C> Key for (A, B, C)
where
    A: Key,
    B: Key,
    C: Key,
{
    const SIZE: KeySize = match (A::SIZE, B::SIZE, C::SIZE) {
        (KeySize::Fixed(s1), KeySize::Fixed(s2), KeySize::Fixed(s3)) => {
            KeySize::Fixed(s1 + s2 + s3)
        }
        _ => KeySize::Variable,
    };

    type OwnedKey = (A::OwnedKey, B::OwnedKey, C::OwnedKey);

    type EncodedBytes<'a>
        = IVec
    where
        Self: 'a;

    fn encode(&self) -> Self::EncodedBytes<'_> {
        match Self::SIZE {
            KeySize::Fixed(s) => {
                let mut out = IVec::with_capacity(s);
                out.extend_from_slice(self.0.encode().as_ref());
                out.extend_from_slice(self.1.encode().as_ref());
                out.extend_from_slice(self.2.encode().as_ref());
                out
            }
            KeySize::Variable => {
                let mut out = IVec::from(self.0.encode().as_ref());
                out.extend_from_slice(self.1.encode().as_ref());
                out.extend_from_slice(self.2.encode().as_ref());
                out
            }
        }
    }

    fn decode_part(bytes: &[u8]) -> (Self::OwnedKey, &[u8]) {
        let (a, r) = A::decode_part(bytes);
        let (b, r) = B::decode_part(r);
        let (c, r) = C::decode_part(r);
        ((a, b, c), r)
    }
}

impl<A, B, C, D> Key for (A, B, C, D)
where
    A: Key,
    B: Key,
    C: Key,
    D: Key,
{
    const SIZE: KeySize = match (A::SIZE, B::SIZE, C::SIZE, D::SIZE) {
        (KeySize::Fixed(s1), KeySize::Fixed(s2), KeySize::Fixed(s3), KeySize::Fixed(s4)) => {
            KeySize::Fixed(s1 + s2 + s3 + s4)
        }
        _ => KeySize::Variable,
    };

    type OwnedKey = (A::OwnedKey, B::OwnedKey, C::OwnedKey, D::OwnedKey);

    type EncodedBytes<'a>
        = IVec
    where
        Self: 'a;

    fn encode(&self) -> Self::EncodedBytes<'_> {
        match Self::SIZE {
            KeySize::Fixed(s) => {
                let mut out = IVec::with_capacity(s);
                out.extend_from_slice(self.0.encode().as_ref());
                out.extend_from_slice(self.1.encode().as_ref());
                out.extend_from_slice(self.2.encode().as_ref());
                out.extend_from_slice(self.3.encode().as_ref());
                out
            }
            KeySize::Variable => {
                let mut out = IVec::from(self.0.encode().as_ref());
                out.extend_from_slice(self.1.encode().as_ref());
                out.extend_from_slice(self.2.encode().as_ref());
                out.extend_from_slice(self.3.encode().as_ref());
                out
            }
        }
    }

    fn decode_part(bytes: &[u8]) -> (Self::OwnedKey, &[u8]) {
        let (a, r) = A::decode_part(bytes);
        let (b, r) = B::decode_part(r);
        let (c, r) = C::decode_part(r);
        let (d, r) = D::decode_part(r);
        ((a, b, c, d), r)
    }
}

/// Appends a primary key to an index key.
///
/// This is used by [`Multi`](crate::Multi) indexes to turn a non-unique logical
/// key into a unique physical store key.
pub trait AppendKey<PK: Key> {
    /// Composite key produced by appending the primary key.
    type Key<'a>: Key
    where
        PK: 'a;

    /// Returns the appended key.
    fn append(self, pk: &PK) -> Self::Key<'_>;
}

impl<K: Key, PK: Key> AppendKey<PK> for (K,) {
    type Key<'a>
        = (K, &'a PK)
    where
        PK: 'a;

    fn append(self, pk: &PK) -> Self::Key<'_> {
        (self.0, pk)
    }
}

impl<A: Key, B: Key, PK: Key> AppendKey<PK> for (A, B) {
    type Key<'a>
        = (A, B, &'a PK)
    where
        PK: 'a;

    fn append(self, pk: &PK) -> Self::Key<'_> {
        (self.0, self.1, pk)
    }
}

impl<A: Key, B: Key, C: Key, PK: Key> AppendKey<PK> for (A, B, C) {
    type Key<'a>
        = (A, B, C, &'a PK)
    where
        PK: 'a;

    fn append(self, pk: &PK) -> Self::Key<'_> {
        (self.0, self.1, self.2, pk)
    }
}

#[cfg(test)]
mod tests {
    use super::Key;
    use crate::inline_vec::IVec;

    #[test]
    fn decode() {
        // u32 — unsigned big-endian
        let cases: &[(&[u8], u32)] = &[
            (&[0x00, 0x00, 0x00, 0x00], 0),
            (&[0x00, 0x00, 0x00, 0x01], 1),
            (&[0xff, 0xff, 0xff, 0xff], u32::MAX),
        ];
        for &(bytes, expected) in cases {
            assert_eq!(u32::decode(bytes), expected, "u32::decode({bytes:02x?})");
        }

        // Vec<u8> — null-escaped, terminated by [0x00, 0x00]
        let cases: &[(&[u8], &[u8])] = &[
            (&[0x00, 0x00], &[]),
            (&[0x01, 0x02, 0x00, 0x00], &[0x01, 0x02]),
            (&[0x00, 0xff, 0x00, 0x00], &[0x00]),
        ];
        for &(bytes, expected) in cases {
            assert_eq!(
                Vec::<u8>::decode(bytes),
                expected,
                "Vec<u8>::decode({bytes:02x?})"
            );
        }

        // panics when extra bytes remain after a fully decoded key
        let panic_cases: &[&dyn Fn()] = &[
            &|| {
                u32::decode(&[0x00, 0x00, 0x00, 0x01, 0xff]);
            },
            &|| {
                bool::decode(&[0x00, 0x01]);
            },
            &|| {
                String::decode(&[b'a', 0xff, b'b']);
            },
        ];
        for &case in panic_cases {
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(case)).is_err(),
                "expected panic for trailing bytes"
            );
        }
    }

    #[test]
    fn encode_unsized_key_bytes() {
        // (input, expected_encoded)
        let cases: &[(&[u8], &[u8])] = &[
            (&[], &[0x00, 0x00]),
            (&[0x01, 0x02, 0x03], &[0x01, 0x02, 0x03, 0x00, 0x00]),
            (&[0xff, 0xfe], &[0xff, 0xfe, 0x00, 0x00]),
            // null bytes are escaped to [0x00, 0xff]
            (&[0x00], &[0x00, 0xff, 0x00, 0x00]),
            (&[0x00, 0x00], &[0x00, 0xff, 0x00, 0xff, 0x00, 0x00]),
            (&[0x01, 0x00, 0x02], &[0x01, 0x00, 0xff, 0x02, 0x00, 0x00]),
        ];

        for (input, expected) in cases {
            let mut out = IVec::new();
            super::encode_unsized_key_bytes(input, &mut out);
            assert_eq!(out.as_ref(), *expected, "encode({input:02x?})");
        }
    }

    #[test]
    fn decode_unsized_key_bytes() {
        // (encoded, expected_decoded, expected_remainder)
        let cases: &[(&[u8], &[u8], &[u8])] = &[
            (&[0x00, 0x00], &[], &[]),
            (&[0x01, 0x02, 0x03, 0x00, 0x00], &[0x01, 0x02, 0x03], &[]),
            (&[0xff, 0xfe, 0x00, 0x00], &[0xff, 0xfe], &[]),
            // escaped null bytes are decoded back to 0x00
            (&[0x00, 0xff, 0x00, 0x00], &[0x00], &[]),
            (&[0x00, 0xff, 0x00, 0xff, 0x00, 0x00], &[0x00, 0x00], &[]),
            (
                &[0x01, 0x00, 0xff, 0x02, 0x00, 0x00],
                &[0x01, 0x00, 0x02],
                &[],
            ),
            // bytes after the terminator are returned as remainder
            (&[0x00, 0x00, 0x42], &[], &[0x42]),
            (&[0x01, 0x00, 0x00, 0x02, 0x03], &[0x01], &[0x02, 0x03]),
        ];

        for (encoded, expected_value, expected_remainder) in cases {
            let (value, remainder) = super::decode_unsized_key_bytes(encoded);
            assert_eq!(value, *expected_value, "decode({encoded:02x?}) value");
            assert_eq!(
                remainder, *expected_remainder,
                "decode({encoded:02x?}) remainder"
            );
        }

        let panic_cases: &[&[u8]] = &[
            &[],           // empty — no terminator
            &[0x01, 0x02], // missing terminator
            &[0x00, 0x01], // invalid escape sequence
        ];

        for &input in panic_cases {
            assert!(
                std::panic::catch_unwind(|| super::decode_unsized_key_bytes(input)).is_err(),
                "expected panic for input {input:02x?}"
            );
        }
    }

    #[test]
    fn encode_decode_round_trip() {
        let cases: &[&[u8]] = &[
            &[],
            &[0x01, 0x02, 0x03],
            &[0xff],
            &[0x00],
            &[0x00, 0x00],
            &[0x01, 0x00, 0x02],
            &[0x00, 0x01, 0x00, 0xff, 0x00],
        ];

        for &input in cases {
            let mut encoded = IVec::new();
            super::encode_unsized_key_bytes(input, &mut encoded);
            let (decoded, remainder) = super::decode_unsized_key_bytes(&encoded);
            assert_eq!(decoded, input, "round-trip({input:02x?})");
            assert!(
                remainder.is_empty(),
                "unexpected remainder after round-trip"
            );
        }
    }

    #[test]
    fn encode_integers() {
        // u8 — big-endian (1 byte)
        let cases: &[(u8, &[u8])] = &[(0, &[0x00]), (1, &[0x01]), (u8::MAX, &[0xff])];
        for &(value, expected) in cases {
            assert_eq!(value.encode().as_ref(), expected, "u8::encode({value})");
        }

        // i8 — XOR with 0x80 to flip sign bit, then big-endian
        let cases: &[(i8, &[u8])] = &[
            (i8::MIN, &[0x00]),
            (-1, &[0x7f]),
            (0, &[0x80]),
            (1, &[0x81]),
            (i8::MAX, &[0xff]),
        ];
        for &(value, expected) in cases {
            assert_eq!(value.encode().as_ref(), expected, "i8::encode({value})");
        }

        // u128 — big-endian (16 bytes)
        let one_u128 = {
            let mut b = [0u8; 16];
            b[15] = 0x01;
            b
        };
        let cases: &[(u128, [u8; 16])] = &[(0, [0x00; 16]), (1, one_u128), (u128::MAX, [0xff; 16])];
        for &(value, expected) in cases {
            assert_eq!(value.encode().as_ref(), expected, "u128::encode({value})");
        }

        // i128 — XOR with bit 127 to flip sign bit, then big-endian
        let zero_i128 = {
            let mut b = [0u8; 16];
            b[0] = 0x80;
            b
        };
        let neg_one_i128 = {
            let mut b = [0xffu8; 16];
            b[0] = 0x7f;
            b
        };
        let one_i128 = {
            let mut b = [0u8; 16];
            b[0] = 0x80;
            b[15] = 0x01;
            b
        };
        let cases: &[(i128, [u8; 16])] = &[
            (i128::MIN, [0x00; 16]),
            (-1, neg_one_i128),
            (0, zero_i128),
            (1, one_i128),
            (i128::MAX, [0xff; 16]),
        ];
        for &(value, expected) in cases {
            assert_eq!(value.encode().as_ref(), expected, "i128::encode({value})");
        }
    }

    #[test]
    fn decode_part_integers() {
        // u8 — exact bytes and trailing remainder
        let cases: &[(&[u8], u8, &[u8])] = &[
            (&[0x00], 0, &[]),
            (&[0x01], 1, &[]),
            (&[0xff], u8::MAX, &[]),
            (&[0x42, 0xde, 0xad], 0x42, &[0xde, 0xad]),
        ];
        for &(bytes, expected, remainder) in cases {
            let (value, rest) = u8::decode_part(bytes);
            assert_eq!(value, expected, "u8::decode_part({bytes:02x?}) value");
            assert_eq!(rest, remainder, "u8::decode_part({bytes:02x?}) remainder");
        }

        // i8 — XOR-flipped encoding
        let cases: &[(&[u8], i8, &[u8])] = &[
            (&[0x00], i8::MIN, &[]),
            (&[0x7f], -1, &[]),
            (&[0x80], 0, &[]),
            (&[0x81], 1, &[]),
            (&[0xff], i8::MAX, &[]),
            (&[0x80, 0xde, 0xad], 0, &[0xde, 0xad]),
        ];
        for &(bytes, expected, remainder) in cases {
            let (value, rest) = i8::decode_part(bytes);
            assert_eq!(value, expected, "i8::decode_part({bytes:02x?}) value");
            assert_eq!(rest, remainder, "i8::decode_part({bytes:02x?}) remainder");
        }

        // u128 — big-endian (16 bytes)
        let one_bytes = {
            let mut b = [0u8; 16];
            b[15] = 0x01;
            b
        };
        let u128_cases: &[(&[u8], u128, &[u8])] = &[
            (&[0x00u8; 16], 0, &[]),
            (&one_bytes, 1, &[]),
            (&[0xffu8; 16], u128::MAX, &[]),
        ];
        for &(bytes, expected, remainder) in u128_cases {
            let (value, rest) = u128::decode_part(bytes);
            assert_eq!(value, expected, "u128::decode_part value");
            assert_eq!(rest, remainder, "u128::decode_part remainder");
        }
        // with trailing bytes
        let bytes_with_tail: Vec<u8> = [0u8; 16].iter().chain(&[0xca, 0xfe]).copied().collect();
        let (value, rest) = u128::decode_part(&bytes_with_tail);
        assert_eq!(value, 0u128);
        assert_eq!(rest, &[0xca, 0xfe]);

        // i128 — XOR-flipped big-endian (16 bytes)
        let zero_enc = {
            let mut b = [0u8; 16];
            b[0] = 0x80;
            b
        };
        let neg_one_enc = {
            let mut b = [0xffu8; 16];
            b[0] = 0x7f;
            b
        };
        let i128_cases: &[(&[u8], i128, &[u8])] = &[
            (&[0x00u8; 16], i128::MIN, &[]),
            (&neg_one_enc, -1, &[]),
            (&zero_enc, 0, &[]),
            (&[0xffu8; 16], i128::MAX, &[]),
        ];
        for &(bytes, expected, remainder) in i128_cases {
            let (value, rest) = i128::decode_part(bytes);
            assert_eq!(value, expected, "i128::decode_part value");
            assert_eq!(rest, remainder, "i128::decode_part remainder");
        }
        // with trailing bytes
        let bytes_with_tail: Vec<u8> = [0x00u8; 16].iter().chain(&[0xbe, 0xef]).copied().collect();
        let (value, rest) = i128::decode_part(&bytes_with_tail);
        assert_eq!(value, i128::MIN);
        assert_eq!(rest, &[0xbe, 0xef]);
    }

    #[test]
    fn encode_bool() {
        let cases: &[(bool, &[u8])] = &[(false, &[0x00]), (true, &[0x01])];
        for &(value, expected) in cases {
            assert_eq!(value.encode().as_ref(), expected, "bool::encode({value})");
        }
    }

    #[test]
    fn decode_part_bool() {
        let cases: &[(&[u8], bool, &[u8])] = &[
            (&[0x00], false, &[]),
            (&[0x01], true, &[]),
            (&[0x00, 0xde, 0xad], false, &[0xde, 0xad]),
            (&[0x01, 0xca, 0xfe], true, &[0xca, 0xfe]),
        ];
        for &(bytes, expected, remainder) in cases {
            let (value, rest) = bool::decode_part(bytes);
            assert_eq!(value, expected, "bool::decode_part({bytes:02x?}) value");
            assert_eq!(rest, remainder, "bool::decode_part({bytes:02x?}) remainder");
        }

        let panic_cases: &[&[u8]] = &[
            &[],     // empty input
            &[0x02], // invalid discriminant
            &[0xff], // invalid discriminant
        ];
        for &input in panic_cases {
            assert!(
                std::panic::catch_unwind(|| bool::decode_part(input)).is_err(),
                "expected panic for input {input:02x?}"
            );
        }
    }

    #[test]
    fn encode_string() {
        // UTF-8 bytes followed by 0xff terminator (0xff is never a valid UTF-8 byte)
        let cases: &[(&str, &[u8])] = &[
            ("", &[0xff]),
            ("hi", &[0x68, 0x69, 0xff]),
            ("héllo", &[0x68, 0xc3, 0xa9, 0x6c, 0x6c, 0x6f, 0xff]),
        ];
        for &(value, expected) in cases {
            assert_eq!(
                value.to_string().encode().as_slice(),
                expected,
                "String::encode({value:?})"
            );
        }
    }

    #[test]
    fn decode_part_string() {
        let cases: &[(&[u8], &str, &[u8])] = &[
            (&[0xff], "", &[]),
            (&[0x68, 0x69, 0xff], "hi", &[]),
            (&[0x68, 0xc3, 0xa9, 0x6c, 0x6c, 0x6f, 0xff], "héllo", &[]),
            // bytes after the terminator become the remainder
            (&[0x68, 0x69, 0xff, 0x01, 0x02], "hi", &[0x01, 0x02]),
            (&[0xff, 0xde, 0xad], "", &[0xde, 0xad]),
        ];
        for &(bytes, expected, remainder) in cases {
            let (value, rest) = String::decode_part(bytes);
            assert_eq!(value, expected, "String::decode_part({bytes:02x?}) value");
            assert_eq!(
                rest, remainder,
                "String::decode_part({bytes:02x?}) remainder"
            );
        }

        let panic_cases: &[&[u8]] = &[
            &[],           // empty — no terminator
            &[0x68, 0x69], // missing 0xff terminator
            &[0x80, 0xff], // invalid UTF-8 (lone continuation byte)
        ];
        for &input in panic_cases {
            assert!(
                std::panic::catch_unwind(|| String::decode_part(input)).is_err(),
                "expected panic for input {input:02x?}"
            );
        }
    }

    #[test]
    fn encode_array_u8() {
        // [u8; S] encodes as its raw bytes — no framing, no escaping
        let cases: &[([u8; 3], &[u8])] = &[
            ([0x00, 0x00, 0x00], &[0x00, 0x00, 0x00]),
            ([0x01, 0x02, 0x03], &[0x01, 0x02, 0x03]),
            ([0x00, 0xff, 0x00], &[0x00, 0xff, 0x00]),
        ];
        for &(value, expected) in cases {
            assert_eq!(
                value.encode().as_ref(),
                expected,
                "[u8;3]::encode({value:02x?})"
            );
        }
    }

    #[test]
    fn decode_part_array_u8() {
        let cases: &[(&[u8], [u8; 3], &[u8])] = &[
            (&[0x01, 0x02, 0x03], [0x01, 0x02, 0x03], &[]),
            (&[0x00, 0xff, 0x00], [0x00, 0xff, 0x00], &[]),
            // trailing bytes become the remainder
            (
                &[0x01, 0x02, 0x03, 0xde, 0xad],
                [0x01, 0x02, 0x03],
                &[0xde, 0xad],
            ),
        ];
        for &(bytes, expected, remainder) in cases {
            let (value, rest) = <[u8; 3]>::decode_part(bytes);
            assert_eq!(value, expected, "[u8;3]::decode_part({bytes:02x?}) value");
            assert_eq!(
                rest, remainder,
                "[u8;3]::decode_part({bytes:02x?}) remainder"
            );
        }

        // panics when fewer bytes than S are available
        let panic_cases: &[&[u8]] = &[
            &[],           // empty
            &[0x01],       // 1 byte, needs 3
            &[0x01, 0x02], // 2 bytes, needs 3
        ];
        for &input in panic_cases {
            assert!(
                std::panic::catch_unwind(|| <[u8; 3]>::decode_part(input)).is_err(),
                "expected panic for input {input:02x?}"
            );
        }
    }

    #[test]
    fn encode_vec_u8() {
        // Vec<u8> uses null-escaping: 0x00 → [0x00, 0xff], terminated by [0x00, 0x00]
        let cases: &[(&[u8], &[u8])] = &[
            (&[], &[0x00, 0x00]),
            (&[0x01, 0x02], &[0x01, 0x02, 0x00, 0x00]),
            (&[0x00], &[0x00, 0xff, 0x00, 0x00]),
            (
                &[0x00, 0x01, 0x00],
                &[0x00, 0xff, 0x01, 0x00, 0xff, 0x00, 0x00],
            ),
        ];
        for &(input, expected) in cases {
            assert_eq!(
                input.to_vec().encode().as_slice(),
                expected,
                "Vec<u8>::encode({input:02x?})"
            );
        }
    }

    #[test]
    fn decode_part_vec_u8() {
        let cases: &[(&[u8], &[u8], &[u8])] = &[
            (&[0x00, 0x00], &[], &[]),
            (&[0x01, 0x02, 0x00, 0x00], &[0x01, 0x02], &[]),
            (&[0x00, 0xff, 0x00, 0x00], &[0x00], &[]),
            (
                &[0x00, 0xff, 0x01, 0x00, 0xff, 0x00, 0x00],
                &[0x00, 0x01, 0x00],
                &[],
            ),
            // bytes after the terminator become the remainder
            (&[0x01, 0x00, 0x00, 0xde, 0xad], &[0x01], &[0xde, 0xad]),
        ];
        for &(bytes, expected, remainder) in cases {
            let (value, rest) = Vec::<u8>::decode_part(bytes);
            assert_eq!(value, expected, "Vec<u8>::decode_part({bytes:02x?}) value");
            assert_eq!(
                rest, remainder,
                "Vec<u8>::decode_part({bytes:02x?}) remainder"
            );
        }

        let panic_cases: &[&[u8]] = &[
            &[],           // empty — no terminator
            &[0x01, 0x02], // missing terminator
            &[0x00, 0x01], // invalid escape sequence
        ];
        for &input in panic_cases {
            assert!(
                std::panic::catch_unwind(|| Vec::<u8>::decode_part(input)).is_err(),
                "expected panic for input {input:02x?}"
            );
        }
    }

    #[test]
    fn encode_tuples() {
        // (A, B) fixed-size: fields concatenated in order
        let cases: &[((u32, u8), &[u8])] = &[
            ((0, 0), &[0x00, 0x00, 0x00, 0x00, 0x00]),
            ((1, 2), &[0x00, 0x00, 0x00, 0x01, 0x02]),
            ((u32::MAX, u8::MAX), &[0xff, 0xff, 0xff, 0xff, 0xff]),
        ];
        for (value, expected) in cases {
            assert_eq!(
                value.encode().as_slice(),
                *expected,
                "(u32,u8)::encode({value:?})"
            );
        }

        // (A, B) variable-size: variable-field framing is preserved
        assert_eq!(
            ("hi".to_string(), 1u8).encode().as_slice(),
            &[0x68, 0x69, 0xff, 0x01]
        );
        assert_eq!(
            (1u8, vec![0x00u8, 0x42]).encode().as_slice(),
            &[0x01, 0x00, 0xff, 0x42, 0x00, 0x00]
        );

        // (A, B, C) fixed-size
        let cases: &[((u8, bool, u8), &[u8])] = &[
            ((1, false, 2), &[0x01, 0x00, 0x02]),
            ((0, true, 255), &[0x00, 0x01, 0xff]),
        ];
        for (value, expected) in cases {
            assert_eq!(
                value.encode().as_slice(),
                *expected,
                "(u8,bool,u8)::encode({value:?})"
            );
        }

        // (A, B, C) variable-size: Vec<u8> in the middle, framing separates it from last field
        assert_eq!(
            (1u8, vec![0x42u8], 2u8).encode().as_slice(),
            &[0x01, 0x42, 0x00, 0x00, 0x02]
        );

        // (A, B, C, D) fixed-size
        let cases: &[((u8, bool, u8, bool), &[u8])] = &[
            ((1, true, 2, false), &[0x01, 0x01, 0x02, 0x00]),
            ((0, false, 0, false), &[0x00, 0x00, 0x00, 0x00]),
        ];
        for (value, expected) in cases {
            assert_eq!(
                value.encode().as_slice(),
                *expected,
                "(u8,bool,u8,bool)::encode({value:?})"
            );
        }

        // (A, B, C, D) variable-size: String in second position
        assert_eq!(
            (1u8, "hi".to_string(), 2u8, true).encode().as_slice(),
            &[0x01, 0x68, 0x69, 0xff, 0x02, 0x01]
        );
    }

    #[test]
    fn decode_part_tuples() {
        // (A, B) fixed-size: remainder threaded correctly
        let cases: &[(&[u8], (u32, u8), &[u8])] = &[
            (&[0x00, 0x00, 0x00, 0x01, 0x02], (1, 2), &[]),
            (&[0xff, 0xff, 0xff, 0xff, 0xff], (u32::MAX, u8::MAX), &[]),
            (
                &[0x00, 0x00, 0x00, 0x01, 0x02, 0xde, 0xad],
                (1, 2),
                &[0xde, 0xad],
            ),
        ];
        for &(bytes, expected, remainder) in cases {
            let (value, rest) = <(u32, u8)>::decode_part(bytes);
            assert_eq!(value, expected, "(u32,u8)::decode_part({bytes:02x?}) value");
            assert_eq!(
                rest, remainder,
                "(u32,u8)::decode_part({bytes:02x?}) remainder"
            );
        }

        // (A, B) variable-size: variable-field framing consumed, remainder is correct
        let (value, rest) = <(String, u8)>::decode_part(&[0x68, 0x69, 0xff, 0x01, 0xde, 0xad]);
        assert_eq!(value, ("hi".to_string(), 1u8));
        assert_eq!(rest, &[0xde, 0xad]);

        // (A, B, C) fixed-size
        let cases: &[(&[u8], (u8, bool, u8), &[u8])] = &[
            (&[0x01, 0x00, 0x02], (1, false, 2), &[]),
            (
                &[0x00, 0x01, 0xff, 0xca, 0xfe],
                (0, true, 255),
                &[0xca, 0xfe],
            ),
        ];
        for &(bytes, expected, remainder) in cases {
            let (value, rest) = <(u8, bool, u8)>::decode_part(bytes);
            assert_eq!(
                value, expected,
                "(u8,bool,u8)::decode_part({bytes:02x?}) value"
            );
            assert_eq!(
                rest, remainder,
                "(u8,bool,u8)::decode_part({bytes:02x?}) remainder"
            );
        }

        // (A, B, C) variable-size: framing of middle field cleanly separates fields
        let (value, rest) =
            <(u8, Vec<u8>, u8)>::decode_part(&[0x01, 0x42, 0x00, 0x00, 0x02, 0xbe, 0xef]);
        assert_eq!(value, (1u8, vec![0x42u8], 2u8));
        assert_eq!(rest, &[0xbe, 0xef]);

        // (A, B, C, D) fixed-size
        let cases: &[(&[u8], (u8, bool, u8, bool), &[u8])] = &[
            (&[0x01, 0x01, 0x02, 0x00], (1, true, 2, false), &[]),
            (
                &[0x00, 0x00, 0x00, 0x00, 0xab],
                (0, false, 0, false),
                &[0xab],
            ),
        ];
        for &(bytes, expected, remainder) in cases {
            let (value, rest) = <(u8, bool, u8, bool)>::decode_part(bytes);
            assert_eq!(
                value, expected,
                "(u8,bool,u8,bool)::decode_part({bytes:02x?}) value"
            );
            assert_eq!(
                rest, remainder,
                "(u8,bool,u8,bool)::decode_part({bytes:02x?}) remainder"
            );
        }

        // (A, B, C, D) variable-size
        let (value, rest) =
            <(u8, String, u8, bool)>::decode_part(&[0x01, 0x68, 0x69, 0xff, 0x02, 0x01, 0xee]);
        assert_eq!(value, (1u8, "hi".to_string(), 2u8, true));
        assert_eq!(rest, &[0xee]);
    }

    #[test]
    fn encode_ref_key() {
        // &K encodes identically to K — no extra bytes, SIZE unchanged
        assert_eq!((&1u32).encode().as_ref(), 1u32.encode().as_ref());
        assert_eq!((&true).encode().as_ref(), true.encode().as_ref());

        // variable-size types: lifetime chains through EncodedBytes correctly
        let s = "hi".to_string();
        assert_eq!((&s).encode().as_slice(), s.encode().as_slice());

        let v = vec![0x00u8, 0x42];
        assert_eq!((&v).encode().as_slice(), v.encode().as_slice());
    }

    #[test]
    fn decode_part_ref_key() {
        // &K::decode_part delegates to K::decode_part — decodes to OwnedKey, not a reference
        let cases: &[(&[u8], u32, &[u8])] = &[
            (&[0x00, 0x00, 0x00, 0x01], 1, &[]),
            (&[0x00, 0x00, 0x00, 0x01, 0xde, 0xad], 1, &[0xde, 0xad]),
        ];
        for &(bytes, expected, remainder) in cases {
            let (value, rest) = <&u32>::decode_part(bytes);
            assert_eq!(value, expected, "&u32::decode_part({bytes:02x?}) value");
            assert_eq!(rest, remainder, "&u32::decode_part({bytes:02x?}) remainder");
        }

        // variable-size type: same framing as String::decode_part
        let (value, rest) = <&String>::decode_part(&[0x68, 0x69, 0xff, 0x01]);
        assert_eq!(value, "hi");
        assert_eq!(rest, &[0x01]);
    }

    #[test]
    fn encode_single_tuple_key() {
        // (K,) encodes identically to K — same bytes, no wrapper overhead
        assert_eq!((1u32,).encode().as_ref(), 1u32.encode().as_ref());
        assert_eq!((true,).encode().as_ref(), true.encode().as_ref());

        // variable-size types
        assert_eq!(
            ("hi".to_string(),).encode().as_slice(),
            "hi".to_string().encode().as_slice()
        );
        assert_eq!(
            (vec![0x00u8, 0x42],).encode().as_slice(),
            vec![0x00u8, 0x42].encode().as_slice()
        );
    }

    #[test]
    fn decode_part_single_tuple_key() {
        // (K,)::decode_part wraps the decoded value in a 1-tuple — distinct from K::decode_part
        let cases: &[(&[u8], (u32,), &[u8])] = &[
            (&[0x00, 0x00, 0x00, 0x01], (1,), &[]),
            (&[0x00, 0x00, 0x00, 0x01, 0xde, 0xad], (1,), &[0xde, 0xad]),
        ];
        for &(bytes, expected, remainder) in cases {
            let (value, rest) = <(u32,)>::decode_part(bytes);
            assert_eq!(value, expected, "(u32,)::decode_part({bytes:02x?}) value");
            assert_eq!(
                rest, remainder,
                "(u32,)::decode_part({bytes:02x?}) remainder"
            );
        }

        // variable-size type: framing consumed, result wrapped in 1-tuple
        let (value, rest) = <(String,)>::decode_part(&[0x68, 0x69, 0xff, 0x01]);
        assert_eq!(value, ("hi".to_string(),));
        assert_eq!(rest, &[0x01]);
    }

    #[test]
    fn append_key() {
        use super::AppendKey;

        // (K,) → (K, &PK): 1-element index key with PK appended
        let cases: &[((u32,), u8, &[u8])] = &[
            ((0,), 0, &[0x00, 0x00, 0x00, 0x00, 0x00]),
            ((1,), 2, &[0x00, 0x00, 0x00, 0x01, 0x02]),
            ((u32::MAX,), u8::MAX, &[0xff, 0xff, 0xff, 0xff, 0xff]),
        ];
        for (index_key, pk, expected) in cases {
            assert_eq!(
                index_key.append(pk).encode().as_slice(),
                *expected,
                "({index_key:?}).append({pk}) encode"
            );
        }

        // (K,) with variable-size PK: Vec<u8> as primary key
        assert_eq!(
            (1u8,).append(&vec![0x02u8, 0x03]).encode().as_slice(),
            &[0x01, 0x02, 0x03, 0x00, 0x00]
        );

        // (A, B) → (A, B, &PK): 2-element index key with PK appended
        let cases: &[((u8, bool), u32, &[u8])] = &[
            ((1, false), 0, &[0x01, 0x00, 0x00, 0x00, 0x00, 0x00]),
            ((2, true), 100, &[0x02, 0x01, 0x00, 0x00, 0x00, 0x64]),
        ];
        for (index_key, pk, expected) in cases {
            assert_eq!(
                index_key.append(pk).encode().as_slice(),
                *expected,
                "({index_key:?}).append({pk}) encode"
            );
        }

        // (A, B) with variable-size index field and fixed PK
        assert_eq!(
            ("hi".to_string(), 1u8).append(&42u32).encode().as_slice(),
            &[0x68, 0x69, 0xff, 0x01, 0x00, 0x00, 0x00, 0x2a]
        );

        // (A, B, C) → (A, B, C, &PK): 3-element index key with PK appended
        let cases: &[((u8, bool, u8), u8, &[u8])] = &[
            ((1, true, 2), 3, &[0x01, 0x01, 0x02, 0x03]),
            ((0, false, 255), 0, &[0x00, 0x00, 0xff, 0x00]),
        ];
        for (index_key, pk, expected) in cases {
            assert_eq!(
                index_key.append(pk).encode().as_slice(),
                *expected,
                "({index_key:?}).append({pk}) encode"
            );
        }

        // (A, B, C) with variable-size PK: String as primary key
        assert_eq!(
            (1u8, true, 2u8)
                .append(&"pk".to_string())
                .encode()
                .as_slice(),
            &[0x01, 0x01, 0x02, 0x70, 0x6b, 0xff]
        );
    }
}
