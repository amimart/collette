use crate::entity::Entity;
use crate::error::Error;
use crate::index::{Index, IndexKind};
use crate::index_registry::{Cons, ContainsIndex, IndexRegistry, Nil};
use crate::key::Key;
use crate::scan::IndexScan;
use crate::store::{
    MultiStore, MultiStoreReadHandle, MultiStoreWriteHandle, ReadKVStore, WriteKVStore,
};
use std::borrow::Borrow;
use std::marker::PhantomData;

pub struct Collection<DB, Record, Indexes>
where
    DB: MultiStore,
    // The stored record implementing the Entity contract
    Record: Entity,
    Indexes: IndexRegistry<Record>,
{
    name: &'static str,
    db: DB,

    _marker: PhantomData<(Record, Indexes)>,
}

impl<DB, Record, Indexes> Collection<DB, Record, Indexes>
where
    DB: MultiStore,
    Record: Entity,
    Indexes: IndexRegistry<Record>,
{
    const MAIN_STORE: &'static str = "__main";

    pub fn new(name: &'static str, db: DB) -> Self {
        Self {
            name,
            db,
            _marker: PhantomData,
        }
    }

    /// Inserts a new record into the collection.
    ///
    /// Returns an error if a record with the same primary key already exists.
    ///
    /// All indexes are updated atomically within the same transaction.
    pub fn insert(&self, value: impl Borrow<Record>) -> Result<(), Error> {
        let value = value.borrow();
        let pk = value.key();
        let enc_pk = pk.encode();

        let mut tx = self.db.write(self.name)?;
        {
            let mut store = tx.open_store(Self::MAIN_STORE)?;

            if store.get(&enc_pk)?.is_some() {
                Err(Error::AlreadyExists(format!("{:?}", pk)))?
            }

            store.set(&enc_pk, &value.to_bytes()?)?;
        }

        Indexes::update(&mut tx, &pk, None, value)?;

        tx.commit().map_err(Error::Backend)
    }

    /// Updates an existing record in the collection.
    ///
    /// Returns an error if the record does not already exist.
    ///
    /// Indexes are automatically updated when indexed fields change.
    pub fn update(&self, value: impl Borrow<Record>) -> Result<(), Error> {
        let value = value.borrow();
        let pk = value.key();
        let enc_pk = pk.encode();

        let mut tx = self.db.write(self.name)?;
        let old = {
            let mut store = tx.open_store(Self::MAIN_STORE)?;

            let old = store
                .get(&enc_pk)?
                .map(|bytes| Record::from_bytes(&bytes).map_err(Error::Codec))
                .transpose()?;

            if old.is_none() {
                Err(Error::NotFound(format!("{:?}", pk)))?
            }

            store.set(&enc_pk, &value.to_bytes()?)?;
            old
        };

        Indexes::update(&mut tx, &pk, old.as_ref(), value)?;

        tx.commit().map_err(Error::Backend)
    }

    /// Saves a record into the collection.
    ///
    /// If the record already exists, it is updated.
    /// Otherwise, a new record is inserted.
    ///
    /// Indexes are updated atomically within the same transaction.
    pub fn save(&self, value: impl Borrow<Record>) -> Result<(), Error> {
        let value = value.borrow();
        let pk = value.key();
        let enc_pk = pk.encode();

        let mut tx = self.db.write(self.name)?;
        let old = {
            let mut store = tx.open_store(Self::MAIN_STORE)?;

            let old = store
                .get(&enc_pk)?
                .map(|bytes| Record::from_bytes(&bytes).map_err(Error::Codec))
                .transpose()?;

            store.set(&enc_pk, &value.to_bytes()?)?;
            old
        };

        Indexes::update(&mut tx, &pk, old.as_ref(), value)?;

        tx.commit().map_err(Error::Backend)
    }

    /// Removes a record from the collection by its primary key.
    ///
    /// If the record exists, all associated index entries are also removed.
    ///
    /// Returns `Ok(())` if the record does not exist.
    pub fn remove<'a>(
        &self,
        key: impl Borrow<<Record::Key<'a> as Key>::OwnedKey>,
    ) -> Result<(), Error>
    where
        Record: 'a,
    {
        let pk = key.borrow();
        let enc_pk = pk.encode();

        let mut tx = self.db.write(self.name)?;
        let record = {
            let mut store = tx.open_store(Self::MAIN_STORE)?;

            let record = store
                .get(enc_pk)?
                .map(|bytes| Record::from_bytes(&bytes).map_err(Error::Codec))
                .transpose()?;

            let record = match record {
                Some(record) => record,
                None => return Ok(()),
            };

            store.remove(key.borrow().encode())?;
            record
        };

        Indexes::remove(&mut tx, &record.key(), &record)?;

        tx.commit().map_err(Error::Backend)
    }

    /// Retrieves a record from the collection by its primary key.
    ///
    /// Returns `Ok(None)` if the record does not exist.
    pub fn get<'a>(
        &self,
        key: impl Borrow<<Record::Key<'a> as Key>::OwnedKey>,
    ) -> Result<Option<Record>, Error>
    where
        Record: 'a,
    {
        self.db
            .read(self.name)?
            .open_store(Self::MAIN_STORE)?
            .get(key.borrow().encode())?
            .map(|bytes| Record::from_bytes(&bytes).map_err(Error::Codec))
            .transpose()
    }

    /// Creates a typed scan over a collection index.
    ///
    /// Scans can be configured with:
    /// - prefixes
    /// - cursors
    /// - ordering direction
    /// - limits
    ///
    /// The returned scan is lazy and does not perform any database access
    /// until iterated.
    pub fn scan<'a, Idx, P>(
        &self,
        _idx: Idx,
    ) -> Result<IndexScan<'a, DB::ReadHandle, Record, Idx>, Error>
    where
        Idx: Index<Record>,
        Idx::Kind<'a>: IndexKind<Idx::Key<'a>, Record::Key<'a>>,
        Indexes: ContainsIndex<Idx, P>,
    {
        Ok(IndexScan::new(Self::MAIN_STORE, self.db.read(self.name)?))
    }
}

pub struct CollectionBuilder<DB, Record, Indexes>
where
    DB: MultiStore,
    Record: Entity,
    Indexes: IndexRegistry<Record>,
{
    name: &'static str,
    db: DB,

    _marker: PhantomData<(Record, Indexes)>,
}

impl<DB, Record, Indexes> CollectionBuilder<DB, Record, Indexes>
where
    DB: MultiStore,
    Record: Entity,
    Indexes: IndexRegistry<Record>,
{
    pub fn with_index<Idx>(self) -> CollectionBuilder<DB, Record, Cons<Idx, Indexes>>
    where
        Idx: Index<Record>,
        for<'ik, 'pk> Idx::Kind<'ik>: IndexKind<Idx::Key<'ik>, Record::Key<'pk>>,
    {
        assert!(
            !Indexes::has_index(Idx::NAME),
            "index with name '{}' already exists in collection '{}'",
            Idx::NAME,
            self.name
        );
        CollectionBuilder {
            name: self.name,
            db: self.db,
            _marker: PhantomData,
        }
    }

    pub fn build(self) -> Collection<DB, Record, Indexes> {
        Collection::new(self.name, self.db)
    }
}

pub fn collection<T, DB>(name: &'static str, db: DB) -> CollectionBuilder<DB, T, Nil>
where
    T: Entity,
    DB: MultiStore,
{
    CollectionBuilder {
        name,
        db,
        _marker: PhantomData,
    }
}

#[cfg(test)]
mod tests {
    use crate::collection::Collection;
    use crate::entity::Entity;
    use crate::error::{CodecError, Error};
    use crate::key::Key;
    use crate::testing::{backend_error, MockDb, SpyRegistry};

    // ── Minimal entity ────────────────────────────────────────────────────────

    struct TestRecord {
        id: u32,
    }

    impl Entity for TestRecord {
        type Key<'a> = u32;

        fn key(&self) -> u32 {
            self.id
        }

        fn to_bytes(&self) -> Result<Vec<u8>, CodecError> {
            Ok(self.id.to_be_bytes().to_vec())
        }

        fn from_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
            let id = u32::from_be_bytes(
                bytes
                    .try_into()
                    .map_err(|_| CodecError::new(std::io::Error::other("bad length")))?,
            );
            Ok(TestRecord { id })
        }
    }

    // ── insert ────────────────────────────────────────────────────────────────

    #[test]
    fn insert() {
        let enc_pk = 1u32.encode().to_vec();
        let enc_val = TestRecord { id: 1 }.to_bytes().unwrap();

        macro_rules! run {
            ($db:expr) => {{
                let db: MockDb = $db;
                let log = db.log();
                let col = Collection::<_, TestRecord, SpyRegistry>::new("col", db);
                let result = col.insert(TestRecord { id: 1 });
                (result, log)
            }};
        }

        struct Case {
            name: &'static str,
            db: MockDb,
            registry_fails: bool,
            expect_result: fn(&Result<(), Error>),
            expect_opens: &'static [&'static str],
            expect_sets: usize,
            expect_committed: bool,
            expect_registry_called: bool,
        }

        let cases = vec![
            Case {
                name: "inserts new record",
                db: MockDb::new(),
                registry_fails: false,
                expect_result: |r| assert!(r.is_ok()),
                expect_opens: &["__main"],
                expect_sets: 1,
                expect_committed: true,
                expect_registry_called: true,
            },
            Case {
                name: "fails when record already exists",
                db: MockDb::new().with_data("__main", enc_pk.clone(), enc_val.clone()),
                registry_fails: false,
                expect_result: |r| assert!(matches!(r, Err(Error::AlreadyExists(_)))),
                expect_opens: &["__main"],
                expect_sets: 0,
                expect_committed: false,
                expect_registry_called: false,
            },
            Case {
                name: "propagates backend error from write()",
                db: MockDb::new().with_write_err(|| backend_error("write failed")),
                registry_fails: false,
                expect_result: |r| assert!(matches!(r, Err(Error::Backend(_)))),
                expect_opens: &[],
                expect_sets: 0,
                expect_committed: false,
                expect_registry_called: false,
            },
            Case {
                name: "propagates backend error from commit()",
                db: MockDb::new().with_commit_err(|| backend_error("commit failed")),
                registry_fails: false,
                expect_result: |r| assert!(matches!(r, Err(Error::Backend(_)))),
                expect_opens: &["__main"],
                expect_sets: 1,
                expect_committed: true,
                expect_registry_called: true,
            },
            Case {
                // set is called before the registry; commit is skipped on registry error
                name: "propagates registry error",
                db: MockDb::new(),
                registry_fails: true,
                expect_result: |r| assert!(matches!(r, Err(Error::Unexpected(_)))),
                expect_opens: &["__main"],
                expect_sets: 1,
                expect_committed: false,
                expect_registry_called: true,
            },
        ];

        for c in cases {
            SpyRegistry::reset();
            SpyRegistry::set_fail(c.registry_fails);
            let (result, log) = run!(c.db);
            let log = log.borrow();

            (c.expect_result)(&result);
            assert_eq!(log.opens.as_slice(), c.expect_opens, "[{}] opens", c.name);
            assert_eq!(log.sets.len(), c.expect_sets, "[{}] sets count", c.name);
            assert_eq!(log.committed, c.expect_committed, "[{}] committed", c.name);
            assert_eq!(
                SpyRegistry::was_update_called(),
                c.expect_registry_called,
                "[{}] registry called",
                c.name
            );
        }

        // Verify the exact bytes written to the main store
        SpyRegistry::reset();
        let (result, log) = run!(MockDb::new());
        assert!(result.is_ok());
        let log = log.borrow();
        assert_eq!(
            log.sets[0].0, enc_pk,
            "set key must be the encoded primary key"
        );
        assert_eq!(
            log.sets[0].1, enc_val,
            "set value must be to_bytes() output"
        );
    }

    // ── update ────────────────────────────────────────────────────────────────

    #[test]
    fn update() {
        let enc_pk = 1u32.encode().to_vec();
        let enc_val = TestRecord { id: 1 }.to_bytes().unwrap();

        let existing_db = || MockDb::new().with_data("__main", enc_pk.clone(), enc_val.clone());

        macro_rules! run {
            ($db:expr) => {{
                let db: MockDb = $db;
                let log = db.log();
                let col = Collection::<_, TestRecord, SpyRegistry>::new("col", db);
                let result = col.update(TestRecord { id: 1 });
                (result, log)
            }};
        }

        struct Case {
            name: &'static str,
            db: MockDb,
            registry_fails: bool,
            expect_result: fn(&Result<(), Error>),
            expect_opens: &'static [&'static str],
            expect_sets: usize,
            expect_committed: bool,
            expect_registry_called: bool,
        }

        let cases = vec![
            Case {
                name: "updates existing record",
                db: existing_db(),
                registry_fails: false,
                expect_result: |r| assert!(r.is_ok()),
                expect_opens: &["__main"],
                expect_sets: 1,
                expect_committed: true,
                expect_registry_called: true,
            },
            Case {
                name: "fails when record not found",
                db: MockDb::new(),
                registry_fails: false,
                expect_result: |r| assert!(matches!(r, Err(Error::NotFound(_)))),
                expect_opens: &["__main"],
                expect_sets: 0,
                expect_committed: false,
                expect_registry_called: false,
            },
            Case {
                name: "propagates backend error from write()",
                db: MockDb::new().with_write_err(|| backend_error("write failed")),
                registry_fails: false,
                expect_result: |r| assert!(matches!(r, Err(Error::Backend(_)))),
                expect_opens: &[],
                expect_sets: 0,
                expect_committed: false,
                expect_registry_called: false,
            },
            Case {
                name: "propagates backend error from commit()",
                db: existing_db().with_commit_err(|| backend_error("commit failed")),
                registry_fails: false,
                expect_result: |r| assert!(matches!(r, Err(Error::Backend(_)))),
                expect_opens: &["__main"],
                expect_sets: 1,
                expect_committed: true,
                expect_registry_called: true,
            },
            Case {
                // from_bytes is called on the stored value before set — codec errors must surface
                name: "propagates codec error from corrupted stored bytes",
                db: MockDb::new().with_data("__main", enc_pk.clone(), vec![0x01]),
                registry_fails: false,
                expect_result: |r| assert!(matches!(r, Err(Error::Codec(_)))),
                expect_opens: &["__main"],
                expect_sets: 0,
                expect_committed: false,
                expect_registry_called: false,
            },
            Case {
                // set is called before the registry; commit is skipped on registry error
                name: "propagates registry error",
                db: existing_db(),
                registry_fails: true,
                expect_result: |r| assert!(matches!(r, Err(Error::Unexpected(_)))),
                expect_opens: &["__main"],
                expect_sets: 1,
                expect_committed: false,
                expect_registry_called: true,
            },
        ];

        for c in cases {
            SpyRegistry::reset();
            SpyRegistry::set_fail(c.registry_fails);
            let (result, log) = run!(c.db);
            let log = log.borrow();

            (c.expect_result)(&result);
            assert_eq!(log.opens.as_slice(), c.expect_opens, "[{}] opens", c.name);
            assert_eq!(log.sets.len(), c.expect_sets, "[{}] sets count", c.name);
            assert_eq!(log.committed, c.expect_committed, "[{}] committed", c.name);
            assert_eq!(
                SpyRegistry::was_update_called(),
                c.expect_registry_called,
                "[{}] registry called",
                c.name
            );
        }

        // Verify the exact bytes written to the main store
        SpyRegistry::reset();
        let (result, log) = run!(existing_db());
        assert!(result.is_ok());
        let log = log.borrow();
        assert_eq!(
            log.sets[0].0, enc_pk,
            "set key must be the encoded primary key"
        );
        assert_eq!(
            log.sets[0].1, enc_val,
            "set value must be to_bytes() output"
        );
    }

    // ── save ──────────────────────────────────────────────────────────────────

    #[test]
    fn save() {
        let enc_pk = 1u32.encode().to_vec();
        let enc_val = TestRecord { id: 1 }.to_bytes().unwrap();

        let existing_db = || MockDb::new().with_data("__main", enc_pk.clone(), enc_val.clone());

        macro_rules! run {
            ($db:expr) => {{
                let db: MockDb = $db;
                let log = db.log();
                let col = Collection::<_, TestRecord, SpyRegistry>::new("col", db);
                let result = col.save(TestRecord { id: 1 });
                (result, log)
            }};
        }

        struct Case {
            name: &'static str,
            db: MockDb,
            registry_fails: bool,
            expect_result: fn(&Result<(), Error>),
            expect_opens: &'static [&'static str],
            expect_sets: usize,
            expect_committed: bool,
            expect_registry_called: bool,
        }

        let cases = vec![
            Case {
                name: "save when record does not exist",
                db: MockDb::new(),
                registry_fails: false,
                expect_result: |r| assert!(r.is_ok()),
                expect_opens: &["__main"],
                expect_sets: 1,
                expect_committed: true,
                expect_registry_called: true,
            },
            Case {
                name: "overwrites when record already exists",
                db: existing_db(),
                registry_fails: false,
                expect_result: |r| assert!(r.is_ok()),
                expect_opens: &["__main"],
                expect_sets: 1,
                expect_committed: true,
                expect_registry_called: true,
            },
            Case {
                name: "propagates backend error from write()",
                db: MockDb::new().with_write_err(|| backend_error("write failed")),
                registry_fails: false,
                expect_result: |r| assert!(matches!(r, Err(Error::Backend(_)))),
                expect_opens: &[],
                expect_sets: 0,
                expect_committed: false,
                expect_registry_called: false,
            },
            Case {
                name: "propagates backend error from commit()",
                db: existing_db().with_commit_err(|| backend_error("commit failed")),
                registry_fails: false,
                expect_result: |r| assert!(matches!(r, Err(Error::Backend(_)))),
                expect_opens: &["__main"],
                expect_sets: 1,
                expect_committed: true,
                expect_registry_called: true,
            },
            Case {
                // from_bytes is called on any stored value before set — codec errors must surface
                name: "propagates codec error from corrupted stored bytes",
                db: MockDb::new().with_data("__main", enc_pk.clone(), vec![0x01]),
                registry_fails: false,
                expect_result: |r| assert!(matches!(r, Err(Error::Codec(_)))),
                expect_opens: &["__main"],
                expect_sets: 0,
                expect_committed: false,
                expect_registry_called: false,
            },
            Case {
                // set is called before the registry; commit is skipped on registry error
                name: "propagates registry error",
                db: existing_db(),
                registry_fails: true,
                expect_result: |r| assert!(matches!(r, Err(Error::Unexpected(_)))),
                expect_opens: &["__main"],
                expect_sets: 1,
                expect_committed: false,
                expect_registry_called: true,
            },
        ];

        for c in cases {
            SpyRegistry::reset();
            SpyRegistry::set_fail(c.registry_fails);
            let (result, log) = run!(c.db);
            let log = log.borrow();

            (c.expect_result)(&result);
            assert_eq!(log.opens.as_slice(), c.expect_opens, "[{}] opens", c.name);
            assert_eq!(log.sets.len(), c.expect_sets, "[{}] sets count", c.name);
            assert_eq!(log.committed, c.expect_committed, "[{}] committed", c.name);
            assert_eq!(
                SpyRegistry::was_update_called(),
                c.expect_registry_called,
                "[{}] registry called",
                c.name
            );
        }

        // Verify the exact bytes written to the main store
        SpyRegistry::reset();
        let (result, log) = run!(MockDb::new());
        assert!(result.is_ok());
        let log = log.borrow();
        assert_eq!(
            log.sets[0].0, enc_pk,
            "set key must be the encoded primary key"
        );
        assert_eq!(
            log.sets[0].1, enc_val,
            "set value must be to_bytes() output"
        );
    }

    // ── remove ────────────────────────────────────────────────────────────────

    #[test]
    fn remove() {
        let enc_pk = 1u32.encode().to_vec();
        let enc_val = TestRecord { id: 1 }.to_bytes().unwrap();

        let existing_db = || MockDb::new().with_data("__main", enc_pk.clone(), enc_val.clone());

        macro_rules! run {
            ($db:expr) => {{
                let db: MockDb = $db;
                let log = db.log();
                let col = Collection::<_, TestRecord, SpyRegistry>::new("col", db);
                let result = col.remove(1u32);
                (result, log)
            }};
        }

        struct Case {
            name: &'static str,
            db: MockDb,
            registry_fails: bool,
            expect_result: fn(&Result<(), Error>),
            expect_opens: &'static [&'static str],
            expect_removes: usize,
            expect_committed: bool,
            expect_registry_called: bool,
        }

        let cases = vec![
            Case {
                name: "removes existing record",
                db: existing_db(),
                registry_fails: false,
                expect_result: |r| assert!(r.is_ok()),
                expect_opens: &["__main"],
                expect_removes: 1,
                expect_committed: true,
                expect_registry_called: true,
            },
            Case {
                // record absent → early Ok(()), no write to store, no registry
                name: "returns ok when record does not exist",
                db: MockDb::new(),
                registry_fails: false,
                expect_result: |r| assert!(r.is_ok()),
                expect_opens: &["__main"],
                expect_removes: 0,
                expect_committed: false,
                expect_registry_called: false,
            },
            Case {
                name: "propagates backend error from write()",
                db: MockDb::new().with_write_err(|| backend_error("write failed")),
                registry_fails: false,
                expect_result: |r| assert!(matches!(r, Err(Error::Backend(_)))),
                expect_opens: &[],
                expect_removes: 0,
                expect_committed: false,
                expect_registry_called: false,
            },
            Case {
                name: "propagates backend error from commit()",
                db: existing_db().with_commit_err(|| backend_error("commit failed")),
                registry_fails: false,
                expect_result: |r| assert!(matches!(r, Err(Error::Backend(_)))),
                expect_opens: &["__main"],
                expect_removes: 1,
                expect_committed: true,
                expect_registry_called: true,
            },
            Case {
                // from_bytes is called on any stored value before remove — codec errors must surface
                name: "propagates codec error from corrupted stored bytes",
                db: MockDb::new().with_data("__main", enc_pk.clone(), vec![0x01]),
                registry_fails: false,
                expect_result: |r| assert!(matches!(r, Err(Error::Codec(_)))),
                expect_opens: &["__main"],
                expect_removes: 0,
                expect_committed: false,
                expect_registry_called: false,
            },
            Case {
                // remove is called before the registry; commit is skipped on registry error
                name: "propagates registry error",
                db: existing_db(),
                registry_fails: true,
                expect_result: |r| assert!(matches!(r, Err(Error::Unexpected(_)))),
                expect_opens: &["__main"],
                expect_removes: 1,
                expect_committed: false,
                expect_registry_called: true,
            },
        ];

        for c in cases {
            SpyRegistry::reset();
            SpyRegistry::set_fail(c.registry_fails);
            let (result, log) = run!(c.db);
            let log = log.borrow();

            (c.expect_result)(&result);
            assert_eq!(log.opens.as_slice(), c.expect_opens, "[{}] opens", c.name);
            assert_eq!(
                log.removes.len(),
                c.expect_removes,
                "[{}] removes count",
                c.name
            );
            assert_eq!(log.committed, c.expect_committed, "[{}] committed", c.name);
            assert_eq!(
                SpyRegistry::was_remove_called(),
                c.expect_registry_called,
                "[{}] registry called",
                c.name
            );
        }

        // Verify the exact key passed to store.remove()
        SpyRegistry::reset();
        let (result, log) = run!(existing_db());
        assert!(result.is_ok());
        let log = log.borrow();
        assert_eq!(
            log.removes[0], enc_pk,
            "remove key must be the encoded primary key"
        );
    }

    // ── get ───────────────────────────────────────────────────────────────────

    #[test]
    fn get() {
        let enc_pk = 1u32.encode().to_vec();
        let enc_val = TestRecord { id: 1 }.to_bytes().unwrap();

        let existing_db = || MockDb::new().with_data("__main", enc_pk.clone(), enc_val.clone());

        macro_rules! run {
            ($db:expr) => {{
                let db: MockDb = $db;
                let log = db.log();
                let col = Collection::<_, TestRecord, SpyRegistry>::new("col", db);
                let result = col.get(1u32);
                (result, log)
            }};
        }

        struct Case {
            name: &'static str,
            db: MockDb,
            expect_result: fn(&Result<Option<TestRecord>, Error>),
            expect_opens: &'static [&'static str],
        }

        let cases = vec![
            Case {
                name: "returns the record when it exists",
                db: existing_db(),
                expect_result: |r| {
                    let record = r.as_ref().unwrap().as_ref().unwrap();
                    assert_eq!(record.id, 1);
                },
                expect_opens: &["__main"],
            },
            Case {
                name: "returns None when record does not exist",
                db: MockDb::new(),
                expect_result: |r| assert!(matches!(r, Ok(None))),
                expect_opens: &["__main"],
            },
            Case {
                name: "propagates backend error from read()",
                db: MockDb::new().with_read_err(|| backend_error("read failed")),
                expect_result: |r| assert!(matches!(r, Err(Error::Backend(_)))),
                expect_opens: &[],
            },
            Case {
                name: "propagates codec error from corrupted stored bytes",
                db: MockDb::new().with_data("__main", enc_pk.clone(), vec![0x01]),
                expect_result: |r| assert!(matches!(r, Err(Error::Codec(_)))),
                expect_opens: &["__main"],
            },
        ];

        for c in cases {
            let (result, log) = run!(c.db);
            let log = log.borrow();

            (c.expect_result)(&result);
            assert_eq!(log.opens.as_slice(), c.expect_opens, "[{}] opens", c.name);
        }

        // Verify the exact key passed to store.get()
        let (_, log) = run!(existing_db());
        let log = log.borrow();
        assert_eq!(
            log.gets[0], enc_pk,
            "get key must be the encoded primary key"
        );
    }
}
