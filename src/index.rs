use crate::bounds::{BoundsEncoder, ExactEncoder, PrefixEncoder};
use crate::error::Error;
use crate::item::Item;
use crate::iter::Cursor;
use crate::key::{AppendKey, Key};
use crate::store::{MultiStoreWriteHandle, ReadKVStore, WriteKVStore};

/// A secondary lookup maintained for a record type that implements [`Item`].
///
/// An index extracts an ordered [`Key`] from a record and chooses an
/// [`IndexKind`] that controls whether that key is unique or can point to many
/// records.
///
/// # Unique index
///
/// ```no_run
/// # use collette::Item;
/// #
/// # struct User {
/// #     id: u64,
/// #     email: String,
/// # }
/// #
/// # impl Item for User {
/// #     type Key<'a> = u64;
/// #     type Error = std::convert::Infallible;
/// #
/// #     fn key(&self) -> Self::Key<'_> {
/// #         self.id
/// #     }
/// #
/// #     fn to_bytes(&self) -> Result<Vec<u8>, Self::Error> {
/// #         Ok(self.email.as_bytes().to_vec())
/// #     }
/// #
/// #     fn from_bytes(bytes: &[u8]) -> Result<Self, Self::Error> {
/// #         Ok(Self {
/// #             id: 0,
/// #             email: String::from_utf8_lossy(bytes).into_owned(),
/// #         })
/// #     }
/// # }
/// #
/// struct ByEmail;
///
/// impl collette::Index<User> for ByEmail {
///     type Key<'a> = &'a str;
///     type Kind<'a> = collette::Unique;
///
///     const NAME: &'static str = "by_email";
///
///     fn key(user: &User) -> Self::Key<'_> {
///         user.email.as_str()
///     }
/// }
/// ```
///
/// # Multi index
///
/// Multi indexes append the record primary key to the stored index key, so
/// several records can share the same extracted value.
///
/// ```no_run
/// # use collette::Item;
/// #
/// # #[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// # enum Status {
/// #     Open,
/// #     Closed,
/// # }
/// #
/// # collette::impl_enum_key!(Status as u8 {
/// #     Status::Open => 0,
/// #     Status::Closed => 1,
/// # });
/// #
/// # struct Task {
/// #     id: u64,
/// #     status: Status,
/// # }
/// #
/// # impl Item for Task {
/// #     type Key<'a> = u64;
/// #     type Error = std::convert::Infallible;
/// #
/// #     fn key(&self) -> Self::Key<'_> {
/// #         self.id
/// #     }
/// #
/// #     fn to_bytes(&self) -> Result<Vec<u8>, Self::Error> {
/// #         Ok(vec![self.status.encode()[0]])
/// #     }
/// #
/// #     fn from_bytes(_bytes: &[u8]) -> Result<Self, Self::Error> {
/// #         Ok(Self {
/// #             id: 0,
/// #             status: Status::Open,
/// #         })
/// #     }
/// # }
/// # use collette::Key;
/// #
/// struct ByStatus;
///
/// impl collette::Index<Task> for ByStatus {
///     type Key<'a> = (Status,);
///     type Kind<'a> = collette::Multi;
///
///     const NAME: &'static str = "by_status";
///
///     fn key(task: &Task) -> Self::Key<'_> {
///         (task.status,)
///     }
/// }
/// ```
pub trait Index<Record: Item> {
    /// The logical index key extracted from a record.
    ///
    /// This type may borrow from the record, which keeps index maintenance
    /// allocation-friendly for string and byte-slice fields.
    type Key<'a>: Key
    where
        Record: 'a;

    /// The physical index layout, usually [`Unique`] or [`Multi`].
    type Kind<'a>: IndexKind<Self::Key<'a>, Record::Key<'a>>
    where
        Record: 'a;

    /// Backend store name for this index.
    const NAME: &'static str;

    /// Extracts the logical index key from a record.
    fn key(record: &Record) -> Self::Key<'_>;

    /// Builds a cursor for this index from a record.
    ///
    /// The cursor uses the physical key layout of the index kind. For
    /// [`Unique`] indexes this is the logical index key; for [`Multi`] indexes
    /// it is the logical index key with the record primary key appended.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let cursor = ByStatus::cursor(&user);
    ///
    /// let next_page = users.index_scan(ByStatus)?
    ///     .after(cursor)
    ///     .iter()?;
    /// ```
    fn cursor<'a>(record: &'a Record) -> Cursor
    where
        Self::Kind<'a>: IndexKind<Self::Key<'a>, Record::Key<'a>>,
    {
        let pk = record.key();
        Cursor::from_key(Self::Kind::store_key(Self::key(record), &pk))
    }

    /// Updates this index after a record insert, save, or update.
    ///
    /// Custom implementations are rarely needed; the default implementation
    /// removes the old stored key when it changes and writes the new stored key.
    fn update<'a, DB: MultiStoreWriteHandle>(
        db: &mut DB,
        pk: &Record::Key<'a>,
        old: Option<&Record>,
        new: &'a Record,
    ) -> Result<(), Error>
    where
        for<'ik, 'pk> Self::Kind<'ik>: IndexKind<Self::Key<'ik>, Record::Key<'pk>>,
    {
        let new_skey = Self::Kind::store_key(Self::key(new), pk);

        let mut store = db.open_store(Self::NAME).map_err(Error::backend)?;
        if let Some(item) = old {
            let old_skey = Self::Kind::store_key(Self::key(item), pk);

            if old_skey == new_skey {
                return Ok(());
            }

            store.remove(old_skey.encode()).map_err(Error::backend)?;
        }

        let skey = new_skey.encode();
        if store.get(&skey).map_err(Error::backend)?.is_some() {
            Err(Error::AlreadyExists(format!("{:?}", new_skey)))?
        }

        store.set(skey, pk.encode()).map_err(Error::backend)?;

        Ok(())
    }

    /// Removes this record's index entry.
    fn remove<'a, DB: MultiStoreWriteHandle>(
        db: &mut DB,
        pk: &Record::Key<'a>,
        item: &'a Record,
    ) -> Result<(), Error> {
        let mut store = db.open_store(Self::NAME).map_err(Error::backend)?;
        let skey = Self::Kind::store_key(Self::key(item), pk);

        store.remove(skey.encode()).map_err(Error::backend)
    }
}

/// The backend key stored for an index entry.
pub type StoreKey<'a, 'b, I, PK, T> =
    <<I as Index<T>>::Kind<'a> as IndexKind<<I as Index<T>>::Key<'b>, PK>>::StoreKey<'a, 'b>;

/// Converts a logical index key into the physical key stored in the backend.
///
/// A [`Unique`] index stores only the logical index key. A [`Multi`] index
/// appends the primary key to keep every index entry distinct.
pub trait IndexKind<IndexKey, PrimaryKey>
where
    IndexKey: Key,
    PrimaryKey: Key,
{
    type StoreKey<'a, 'b>: Key
    where
        IndexKey: 'a,
        PrimaryKey: 'b;

    type BoundsEncoder: BoundsEncoder<IndexKey>;

    /// Builds the physical key used in the index store.
    fn store_key<'a, 'b>(k: IndexKey, pk: &'b PrimaryKey) -> Self::StoreKey<'a, 'b>
    where
        IndexKey: 'a;
}

/// Unique index marker.
///
/// A unique index allows at most one record for each extracted index key.
pub struct Unique;

impl<IndexKey, PrimaryKey> IndexKind<IndexKey, PrimaryKey> for Unique
where
    IndexKey: Key,
    PrimaryKey: Key,
{
    type StoreKey<'a, 'b>
        = IndexKey
    where
        IndexKey: 'a,
        PrimaryKey: 'b;

    type BoundsEncoder = ExactEncoder;

    fn store_key<'a, 'b>(k: IndexKey, _pk: &'b PrimaryKey) -> Self::StoreKey<'a, 'b>
    where
        IndexKey: 'a,
    {
        k
    }
}

/// Multi-value index marker.
///
/// A multi index allows several records to share the same extracted index key
/// by appending the record primary key to each stored index entry.
pub struct Multi;

impl<IndexKey, PrimaryKey> IndexKind<IndexKey, PrimaryKey> for Multi
where
    IndexKey: Key + AppendKey<PrimaryKey>,
    PrimaryKey: Key,
{
    type StoreKey<'a, 'b>
        = <IndexKey as AppendKey<PrimaryKey>>::Key<'b>
    where
        IndexKey: 'a,
        PrimaryKey: 'b;

    type BoundsEncoder = PrefixEncoder;

    fn store_key<'a, 'b>(k: IndexKey, pk: &'b PrimaryKey) -> Self::StoreKey<'a, 'b>
    where
        IndexKey: 'a,
    {
        k.append(pk)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Record {
        id: u32,
        email_id: u16,
        group: u16,
    }

    impl Item for Record {
        type Key<'a> = u32;
        type Error = std::convert::Infallible;

        fn key(&self) -> Self::Key<'_> {
            self.id
        }

        fn to_bytes(&self) -> Result<Vec<u8>, Self::Error> {
            Ok(vec![])
        }

        fn from_bytes(_: &[u8]) -> Result<Self, Self::Error> {
            Ok(Self {
                id: 0,
                email_id: 0,
                group: 0,
            })
        }
    }

    struct ByEmailId;

    impl Index<Record> for ByEmailId {
        type Key<'a> = u16;
        type Kind<'a> = Unique;

        const NAME: &'static str = "by_email_id";

        fn key(record: &Record) -> Self::Key<'_> {
            record.email_id
        }
    }

    struct ByGroup;

    impl Index<Record> for ByGroup {
        type Key<'a> = (u16,);
        type Kind<'a> = Multi;

        const NAME: &'static str = "by_group";

        fn key(record: &Record) -> Self::Key<'_> {
            (record.group,)
        }
    }

    #[test]
    fn unique_index_cursor_uses_logical_index_key() {
        let record = Record {
            id: 42,
            email_id: 7,
            group: 3,
        };

        assert_eq!(
            ByEmailId::cursor(&record).into_vec(),
            7u16.encode().as_ref()
        );
    }

    #[test]
    fn multi_index_cursor_appends_primary_key_to_index_key() {
        let record = Record {
            id: 42,
            email_id: 7,
            group: 3,
        };

        assert_eq!(
            ByGroup::cursor(&record).into_vec(),
            (3u16, 42u32).encode().as_ref()
        );
    }
}
