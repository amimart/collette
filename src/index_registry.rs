use crate::error::Error;
use crate::index::{Index, IndexKind};
use crate::item::Item;
use crate::store::MultiStoreWriteHandle;
use std::marker::PhantomData;

// HList helper types:
pub struct Nil;
pub struct Here;
pub struct There<Tail>(PhantomData<Tail>);
pub struct Cons<Head, Tail>(PhantomData<(Head, Tail)>);

/// IndexRegistry is a recursive HList trait to allow defining multiple indexes as generic types.
pub trait IndexRegistry<T: Item> {
    fn store_names(out: &mut Vec<&'static str>);

    fn update<'a, DB: MultiStoreWriteHandle>(
        db: &mut DB,
        pk: &T::Key<'a>,
        old: Option<&T>,
        new: &'a T,
    ) -> Result<(), Error>;

    fn remove<'a, DB: MultiStoreWriteHandle>(
        db: &mut DB,
        pk: &T::Key<'a>,
        item: &'a T,
    ) -> Result<(), Error>;

    fn has_index(name: &str) -> bool;
}

impl<T> IndexRegistry<T> for Nil
where
    T: Item,
{
    fn store_names(_out: &mut Vec<&'static str>) {}

    fn update<'a, DB: MultiStoreWriteHandle>(
        _db: &mut DB,
        _pk: &T::Key<'a>,
        _old: Option<&T>,
        _new: &'a T,
    ) -> Result<(), Error> {
        Ok(())
    }

    fn remove<'a, DB: MultiStoreWriteHandle>(
        _db: &mut DB,
        _pk: &T::Key<'a>,
        _item: &'a T,
    ) -> Result<(), Error> {
        Ok(())
    }

    fn has_index(_name: &str) -> bool {
        false
    }
}

impl<T, Head, Tail> IndexRegistry<T> for Cons<Head, Tail>
where
    T: Item,
    Head: Index<T>,
    Tail: IndexRegistry<T>,
    for<'ik, 'pk> Head::Kind<'ik>: IndexKind<Head::Key<'ik>, T::Key<'pk>>,
{
    fn store_names(out: &mut Vec<&'static str>) {
        out.push(Head::NAME);
        Tail::store_names(out);
    }

    fn update<'a, DB: MultiStoreWriteHandle>(
        db: &mut DB,
        pk: &T::Key<'a>,
        old: Option<&T>,
        new: &'a T,
    ) -> Result<(), Error> {
        Head::update(db, pk, old, new)?;
        Tail::update(db, pk, old, new)
    }

    fn remove<'a, DB: MultiStoreWriteHandle>(
        db: &mut DB,
        pk: &T::Key<'a>,
        item: &'a T,
    ) -> Result<(), Error> {
        Head::remove(db, pk, item)?;
        Tail::remove(db, pk, item)
    }

    fn has_index(name: &str) -> bool {
        Head::NAME == name || Tail::has_index(name)
    }
}

/// ContainsIndex is used check the presence of an index in the registry HList.
pub trait ContainsIndex<I, Proof> {}

impl<I, Tail> ContainsIndex<I, Here> for Cons<I, Tail> {}

impl<I, Head, Tail, Proof> ContainsIndex<I, There<Proof>> for Cons<Head, Tail> where
    Tail: ContainsIndex<I, Proof>
{
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::error::{BackendError, Error};
    use crate::index::Unique;
    use crate::scan::Direction;
    use crate::store::{
        KVEntry, MultiStoreWriteHandle, ReadKVStore, ReadWriteKVStore, WriteKVStore,
    };
    use std::ops::RangeBounds;

    // ── Minimal item ────────────────────────────────────────────────────────

    #[derive(Clone)]
    struct Record(u32);

    impl Item for Record {
        type Key<'a> = u32;
        type Error = std::io::Error;

        fn key(&self) -> u32 {
            self.0
        }

        fn to_bytes(&self) -> Result<Vec<u8>, Self::Error> {
            todo!()
        }

        fn from_bytes(_bytes: &[u8]) -> Result<Self, Self::Error> {
            todo!()
        }
    }

    // ── Mock indexes ──────────────────────────────────────────────────────────
    // Override set/remove to record invocation via open_store, bypassing the
    // default implementation so the test stays focused on IndexRegistry dispatch.

    struct IndexA;
    struct IndexB;
    struct FailIndex;

    macro_rules! spy_index {
        ($ty:ty, $name:literal) => {
            impl Index<Record> for $ty {
                type Key<'a> = u32;
                type Kind<'a> = Unique;
                const NAME: &'static str = $name;
                fn key(r: &Record) -> u32 {
                    r.0
                }
                fn update<DB: MultiStoreWriteHandle>(
                    db: &mut DB,
                    _pk: &u32,
                    _old: Option<&Record>,
                    _new: &Record,
                ) -> Result<(), Error> {
                    db.open_store(Self::NAME).map_err(Error::backend)?;
                    Ok(())
                }
                fn remove<DB: MultiStoreWriteHandle>(
                    db: &mut DB,
                    _pk: &u32,
                    _item: &Record,
                ) -> Result<(), Error> {
                    db.open_store(Self::NAME).map_err(Error::backend)?;
                    Ok(())
                }
            }
        };
    }

    spy_index!(IndexA, "index_a");
    spy_index!(IndexB, "index_b");

    impl Index<Record> for FailIndex {
        type Key<'a> = u32;
        type Kind<'a> = Unique;
        const NAME: &'static str = "fail";
        fn key(r: &Record) -> u32 {
            r.0
        }
        fn update<DB: MultiStoreWriteHandle>(
            _db: &mut DB,
            _pk: &u32,
            _old: Option<&Record>,
            _new: &Record,
        ) -> Result<(), Error> {
            Err(Error::Unexpected("injected".into()))
        }
        fn remove<DB: MultiStoreWriteHandle>(
            _db: &mut DB,
            _pk: &u32,
            _item: &Record,
        ) -> Result<(), Error> {
            Err(Error::Unexpected("injected".into()))
        }
    }

    // ── Spy write handle ──────────────────────────────────────────────────────
    // Records which store names were opened; that is how we observe index dispatch.

    struct NoopStore;
    struct NoopEntry;

    impl KVEntry for NoopEntry {
        fn key(&self) -> &[u8] {
            &[]
        }

        fn value(&self) -> &[u8] {
            &[]
        }
    }

    impl ReadKVStore for NoopStore {
        type Error = BackendError;
        type Value<'a> = Vec<u8>;
        type Entry = NoopEntry;
        type Iter = std::iter::Empty<Result<NoopEntry, BackendError>>;

        fn get(&self, _: impl AsRef<[u8]>) -> Result<Option<Self::Value<'_>>, BackendError> {
            Ok(None)
        }
        fn scan(
            self,
            _: impl RangeBounds<Vec<u8>>,
            _: Direction,
        ) -> Result<Self::Iter, BackendError> {
            Ok(std::iter::empty())
        }
    }
    impl<'a> WriteKVStore<'a> for NoopStore {
        type Error = BackendError;

        fn set(&mut self, _: impl AsRef<[u8]>, _: impl AsRef<[u8]>) -> Result<(), BackendError> {
            Ok(())
        }
        fn remove(&mut self, _: impl AsRef<[u8]>) -> Result<(), BackendError> {
            Ok(())
        }
    }
    impl<'a> ReadWriteKVStore<'a> for NoopStore {
        type Error = BackendError;
    }

    struct Spy(Vec<String>);

    impl Spy {
        fn new() -> Self {
            Self(Vec::new())
        }
        fn invoked(&self) -> Vec<&str> {
            self.0.iter().map(String::as_str).collect()
        }
    }

    impl MultiStoreWriteHandle for Spy {
        type Error = BackendError;
        type Store<'a> = NoopStore;

        fn open_store(&mut self, name: &'static str) -> Result<NoopStore, BackendError> {
            self.0.push(name.to_string());
            Ok(NoopStore)
        }
        fn commit(self) -> Result<(), BackendError> {
            Ok(())
        }
    }

    // ── has_index ─────────────────────────────────────────────────────────────

    #[test]
    fn has_index() {
        let cases: &[(fn(&str) -> bool, &str, bool)] = &[
            (<Nil as IndexRegistry<Record>>::has_index, "index_a", false),
            (<Nil as IndexRegistry<Record>>::has_index, "", false),
            (
                <Cons<IndexA, Nil> as IndexRegistry<Record>>::has_index,
                "index_a",
                true,
            ),
            (
                <Cons<IndexA, Nil> as IndexRegistry<Record>>::has_index,
                "index_b",
                false,
            ),
            (
                <Cons<IndexA, Cons<IndexB, Nil>> as IndexRegistry<Record>>::has_index,
                "index_a",
                true,
            ),
            (
                <Cons<IndexA, Cons<IndexB, Nil>> as IndexRegistry<Record>>::has_index,
                "index_b",
                true,
            ),
            (
                <Cons<IndexA, Cons<IndexB, Nil>> as IndexRegistry<Record>>::has_index,
                "nonexistent",
                false,
            ),
        ];

        for &(has_index, name, expected) in cases {
            assert_eq!(
                has_index(name),
                expected,
                "has_index({name:?}) should be {expected}"
            );
        }
    }

    // ── set ───────────────────────────────────────────────────────────────────

    #[test]
    fn update() {
        let record = Record(1);
        let pk = 1u32;

        let cases: &[(&dyn Fn(&mut Spy) -> Result<(), Error>, &[&str], bool)] = &[
            (
                &|s| <Nil as IndexRegistry<Record>>::update(s, &pk, None, &record),
                &[],
                false,
            ),
            (
                &|s| {
                    <Cons<IndexA, Cons<IndexB, Nil>> as IndexRegistry<Record>>::update(
                        s, &pk, None, &record,
                    )
                },
                &["index_a", "index_b"],
                false,
            ),
            (
                &|s| {
                    <Cons<FailIndex, Cons<IndexA, Nil>> as IndexRegistry<Record>>::update(
                        s, &pk, None, &record,
                    )
                },
                &[],
                true,
            ),
        ];

        for (invoke, expected_invoked, expect_err) in cases {
            let mut spy = Spy::new();
            let result = invoke(&mut spy);
            assert_eq!(result.is_err(), *expect_err);
            assert_eq!(&spy.invoked(), expected_invoked);
        }
    }

    // ── remove ────────────────────────────────────────────────────────────────

    #[test]
    fn remove() {
        let record = Record(1);
        let pk = 1u32;

        let cases: &[(&dyn Fn(&mut Spy) -> Result<(), Error>, &[&str], bool)] = &[
            (
                &|s| <Nil as IndexRegistry<Record>>::remove(s, &pk, &record),
                &[],
                false,
            ),
            (
                &|s| {
                    <Cons<IndexA, Cons<IndexB, Nil>> as IndexRegistry<Record>>::remove(
                        s, &pk, &record,
                    )
                },
                &["index_a", "index_b"],
                false,
            ),
            (
                &|s| {
                    <Cons<FailIndex, Cons<IndexA, Nil>> as IndexRegistry<Record>>::remove(
                        s, &pk, &record,
                    )
                },
                &[],
                true,
            ),
        ];

        for (invoke, expected_invoked, expect_err) in cases {
            let mut spy = Spy::new();
            let result = invoke(&mut spy);
            assert_eq!(result.is_err(), *expect_err);
            assert_eq!(&spy.invoked(), expected_invoked);
        }
    }

    // ── ContainsIndex ─────────────────────────────────────────────────────────

    fn assert_contains<R, I, P>()
    where
        R: ContainsIndex<I, P>,
    {
    }

    #[test]
    fn contains_index_at_head() {
        assert_contains::<Cons<IndexA, Nil>, IndexA, Here>();
    }

    #[test]
    fn contains_index_in_tail() {
        assert_contains::<Cons<IndexA, Cons<IndexB, Nil>>, IndexB, There<Here>>();
    }

    #[test]
    fn contains_index_tracks_depth() {
        struct IndexC;
        spy_index!(IndexC, "index_c");
        type R = Cons<IndexA, Cons<IndexB, Cons<IndexC, Nil>>>;
        assert_contains::<R, IndexA, Here>();
        assert_contains::<R, IndexB, There<Here>>();
        assert_contains::<R, IndexC, There<There<Here>>>();
    }
}
