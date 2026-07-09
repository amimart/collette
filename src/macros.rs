//! Macros for implementing [`Key`](crate::Key).

/// Implements [`Key`](crate::Key) for an unsigned integer type.
///
/// Values are encoded in big-endian order so byte ordering matches numeric
/// ordering.
#[macro_export]
macro_rules! impl_unsigned_integer_key {
    ($ty:ty) => {
        impl $crate::Key for $ty {
            const SIZE: $crate::KeySize = $crate::KeySize::Fixed(std::mem::size_of::<$ty>());

            type OwnedKey = $ty;

            type EncodedBytes<'a>
                = [u8; std::mem::size_of::<$ty>()]
            where
                Self: 'a;

            fn encode(&self) -> Self::EncodedBytes<'_> {
                self.to_be_bytes()
            }

            fn decode_part(bytes: &[u8]) -> (Self::OwnedKey, &[u8]) {
                let (kbytes, r) = bytes.split_at(std::mem::size_of::<$ty>());
                (<$ty>::from_be_bytes(kbytes.try_into().unwrap()), r)
            }
        }
    };
}

/// Implements [`Key`](crate::Key) for a signed integer type.
///
/// The encoding flips the sign bit and then stores the value as big-endian
/// bytes, preserving signed numeric ordering in lexicographic byte order.
#[macro_export]
macro_rules! impl_signed_integer_key {
    ($signed:ty => $unsigned:ty) => {
        impl $crate::Key for $signed {
            const SIZE: $crate::KeySize = $crate::KeySize::Fixed(std::mem::size_of::<$unsigned>());

            type OwnedKey = $signed;

            type EncodedBytes<'a>
                = [u8; std::mem::size_of::<$unsigned>()]
            where
                Self: 'a;

            fn encode(&self) -> Self::EncodedBytes<'_> {
                let sortable = (*self as $unsigned) ^ <$signed>::MIN as $unsigned;
                sortable.to_be_bytes()
            }

            fn decode_part(bytes: &[u8]) -> (Self::OwnedKey, &[u8]) {
                let (kbytes, r) = bytes.split_at(std::mem::size_of::<$unsigned>());
                let sortable = <$unsigned>::from_be_bytes(kbytes.try_into().unwrap());
                ((sortable ^ <$signed>::MIN as $unsigned) as $signed, r)
            }
        }
    };
}

/// Implements [`Key`](crate::Key) for a C-like enum.
///
/// Each variant is mapped to an explicit integer discriminant. The chosen
/// integer ordering becomes the enum's scan/index ordering.
///
/// ```
/// #[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// enum Status {
///     Queued,
///     Done,
/// }
///
/// collette::impl_enum_key!(Status as u8 {
///     Status::Queued => 0,
///     Status::Done => 1,
/// });
/// ```
#[macro_export]
macro_rules! impl_enum_key {
    ($ty:ty as $int:ty { $($variant:path => $value:expr),+ $(,)? }) => {
        impl $crate::Key for $ty {
            const SIZE: $crate::KeySize = $crate::KeySize::Fixed(std::mem::size_of::<$int>());

            type OwnedKey = Self;

            type EncodedBytes<'a> = [u8; std::mem::size_of::<$int>()]
            where
                Self: 'a;

            fn encode(&self) -> Self::EncodedBytes<'_> {
                let v: $int = match self {
                    $($variant => $value,)+
                };
                v.to_be_bytes()
            }

            fn decode_part(bytes: &[u8]) -> (Self::OwnedKey, &[u8]) {
                let (kbytes, r) = bytes.split_at(std::mem::size_of::<$int>());
                let value = <$int>::from_be_bytes(kbytes.try_into().unwrap());
                (match value {
                    $($value => $variant,)+
                    _ => panic!("invalid enum discriminant {value} for type {}", stringify!($ty)),
                }, r)
            }
        }
    };
}
