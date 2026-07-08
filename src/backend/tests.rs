use crate::scan::Direction;
use crate::store::{
    MultiStore, MultiStoreReadHandle, MultiStoreWriteHandle, ReadKVStore, WriteKVStore,
};
use std::ops::{Bound, RangeBounds};
use std::sync::mpsc;
use std::time::Duration;

#[allow(dead_code)]
pub fn run_multistore_tests<DB: MultiStore + Sync>(make_db: impl Fn() -> DB) {
    basic_operations(&make_db);
    namespace_isolation(&make_db);
    store_isolation(&make_db);
    committed_writes_are_visible(&make_db);
    write_handle_reads_include_uncommitted_writes(&make_db);
    read_handles_keep_stable_snapshots(&make_db);
    multiple_read_handles_ignore_open_write_and_commit(&make_db);
    second_write_waits_for_open_write_to_finish(&make_db);
    multi_store_commits_are_atomic(&make_db);
    scans(&make_db);
}

macro_rules! multistore_contract_tests {
    ($make_db:expr) => {
        mod contract {
            use super::*;

            #[test]
            fn basic_operations() {
                $crate::backend::tests::basic_operations(&$make_db);
            }

            #[test]
            fn namespace_isolation() {
                $crate::backend::tests::namespace_isolation(&$make_db);
            }

            #[test]
            fn store_isolation() {
                $crate::backend::tests::store_isolation(&$make_db);
            }

            #[test]
            fn committed_writes_are_visible() {
                $crate::backend::tests::committed_writes_are_visible(&$make_db);
            }

            #[test]
            fn write_handle_reads_include_uncommitted_writes() {
                $crate::backend::tests::write_handle_reads_include_uncommitted_writes(&$make_db);
            }

            #[test]
            fn read_handles_keep_stable_snapshots() {
                $crate::backend::tests::read_handles_keep_stable_snapshots(&$make_db);
            }

            #[test]
            fn multiple_read_handles_ignore_open_write_and_commit() {
                $crate::backend::tests::multiple_read_handles_ignore_open_write_and_commit(
                    &$make_db,
                );
            }

            #[test]
            fn second_write_waits_for_open_write_to_finish() {
                $crate::backend::tests::second_write_waits_for_open_write_to_finish(&$make_db);
            }

            #[test]
            fn multi_store_commits_are_atomic() {
                $crate::backend::tests::multi_store_commits_are_atomic(&$make_db);
            }

            #[test]
            fn scans() {
                $crate::backend::tests::scans(&$make_db);
            }
        }
    };
}

use crate::prefix::encoded_prefix_range;
pub(crate) use multistore_contract_tests;

pub(crate) fn basic_operations<DB: MultiStore>(make_db: &impl Fn() -> DB) {
    let db = make_db();
    db.prepare("basic", ["items"]).unwrap();

    assert_eq!(get(&db, "basic", "items", b"missing"), None);

    commit_entries(&db, "basic", "items", &[(b"a".to_vec(), b"one".to_vec())]);
    assert_eq!(get(&db, "basic", "items", b"a"), Some(b"one".to_vec()));

    commit_entries(&db, "basic", "items", &[(b"a".to_vec(), b"two".to_vec())]);
    assert_eq!(get(&db, "basic", "items", b"a"), Some(b"two".to_vec()));

    remove_and_commit(&db, "basic", "items", b"a");
    assert_eq!(get(&db, "basic", "items", b"a"), None);

    remove_and_commit(&db, "basic", "items", b"missing");
    assert_eq!(get(&db, "basic", "items", b"missing"), None);
}

pub(crate) fn namespace_isolation<DB: MultiStore>(make_db: &impl Fn() -> DB) {
    let db = make_db();
    db.prepare("left", ["items"]).unwrap();
    db.prepare("right", ["items"]).unwrap();

    commit_entries(
        &db,
        "left",
        "items",
        &[(b"same-key".to_vec(), b"left".to_vec())],
    );
    commit_entries(
        &db,
        "right",
        "items",
        &[(b"same-key".to_vec(), b"right".to_vec())],
    );

    assert_eq!(
        get(&db, "left", "items", b"same-key"),
        Some(b"left".to_vec())
    );
    assert_eq!(
        get(&db, "right", "items", b"same-key"),
        Some(b"right".to_vec())
    );
}

pub(crate) fn store_isolation<DB: MultiStore>(make_db: &impl Fn() -> DB) {
    let db = make_db();
    db.prepare("stores", ["primary", "index"]).unwrap();

    {
        let mut write = db.write("stores").unwrap();
        {
            let mut primary = write.open_store("primary").unwrap();
            primary.set(b"same-key", b"primary").unwrap();
        }
        {
            let mut index = write.open_store("index").unwrap();
            index.set(b"same-key", b"index").unwrap();
        }
        write.commit().unwrap();
    }

    assert_eq!(
        get(&db, "stores", "primary", b"same-key"),
        Some(b"primary".to_vec())
    );
    assert_eq!(
        get(&db, "stores", "index", b"same-key"),
        Some(b"index".to_vec())
    );
}

pub(crate) fn committed_writes_are_visible<DB: MultiStore>(make_db: &impl Fn() -> DB) {
    let db = make_db();
    db.prepare("commits", ["items"]).unwrap();

    commit_entries(
        &db,
        "commits",
        "items",
        &[(b"k".to_vec(), b"committed".to_vec())],
    );

    assert_eq!(
        get(&db, "commits", "items", b"k"),
        Some(b"committed".to_vec())
    );
}

pub(crate) fn write_handle_reads_include_uncommitted_writes<DB: MultiStore>(
    make_db: &impl Fn() -> DB,
) {
    let db = make_db();
    db.prepare("write-reads", ["items"]).unwrap();

    let mut write = db.write("write-reads").unwrap();
    {
        let mut store = write.open_store("items").unwrap();
        store.set(b"k", b"uncommitted").unwrap();
        assert_eq!(store.get(b"k").unwrap(), Some(b"uncommitted".to_vec()));
    }
    write.commit().unwrap();
}

pub(crate) fn read_handles_keep_stable_snapshots<DB: MultiStore>(make_db: &impl Fn() -> DB) {
    let db = make_db();
    db.prepare("snapshots", ["items"]).unwrap();

    let before = db.read("snapshots").unwrap();

    commit_entries(
        &db,
        "snapshots",
        "items",
        &[(b"k".to_vec(), b"after".to_vec())],
    );

    assert_eq!(
        before.open_store("items").unwrap().get(b"k").unwrap(),
        None,
        "existing read handles should keep their pre-commit snapshot"
    );
    assert_eq!(
        get(&db, "snapshots", "items", b"k"),
        Some(b"after".to_vec())
    );
}

pub(crate) fn multiple_read_handles_ignore_open_write_and_commit<DB: MultiStore>(
    make_db: &impl Fn() -> DB,
) {
    let db = make_db();
    db.prepare("mvcc-readers", ["items"]).unwrap();

    commit_entries(
        &db,
        "mvcc-readers",
        "items",
        &[(b"k".to_vec(), b"before".to_vec())],
    );

    let read_a = db.read("mvcc-readers").unwrap();
    let read_b = db.read("mvcc-readers").unwrap();

    let mut write = db.write("mvcc-readers").unwrap();
    {
        let mut items = write.open_store("items").unwrap();
        items.set(b"k", b"after").unwrap();
        items.set(b"new", b"created").unwrap();
    }

    for read in [&read_a, &read_b, &db.read("mvcc-readers").unwrap()] {
        let items = read.open_store("items").unwrap();
        assert_eq!(items.get(b"k").unwrap(), Some(b"before".to_vec()));
        assert_eq!(items.get(b"new").unwrap(), None);
    }

    write.commit().unwrap();

    for read in [&read_a, &read_b] {
        let items = read.open_store("items").unwrap();
        assert_eq!(
            items.get(b"k").unwrap(),
            Some(b"before".to_vec()),
            "existing read handles should not observe later commits"
        );
        assert_eq!(
            items.get(b"new").unwrap(),
            None,
            "existing read handles should not observe newly committed keys"
        );
    }

    let after = db.read("mvcc-readers").unwrap();
    let items = after.open_store("items").unwrap();
    assert_eq!(items.get(b"k").unwrap(), Some(b"after".to_vec()));
    assert_eq!(items.get(b"new").unwrap(), Some(b"created".to_vec()));
}

pub(crate) fn second_write_waits_for_open_write_to_finish<DB>(make_db: &impl Fn() -> DB)
where
    DB: MultiStore + Sync,
{
    let db = make_db();
    db.prepare("single-writer", ["items"]).unwrap();

    let mut first_write = Some(db.write("single-writer").unwrap());
    let (events_tx, events_rx) = mpsc::channel();

    std::thread::scope(|scope| {
        scope.spawn(|| {
            events_tx.send("attempting").unwrap();
            let _second_write = db.write("single-writer").unwrap();
            events_tx.send("opened").unwrap();
        });

        assert_eq!(
            events_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            "attempting"
        );
        assert!(
            matches!(
                events_rx.recv_timeout(Duration::from_millis(50)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ),
            "a second write handle should wait while another write handle is open"
        );

        drop(first_write.take());

        assert_eq!(
            events_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            "opened"
        );
    });
}

pub(crate) fn multi_store_commits_are_atomic<DB: MultiStore>(make_db: &impl Fn() -> DB) {
    let db = make_db();
    db.prepare("atomic", ["primary", "index"]).unwrap();

    let before = db.read("atomic").unwrap();

    let mut write = db.write("atomic").unwrap();
    {
        let mut primary = write.open_store("primary").unwrap();
        primary.set(b"id:1", b"record").unwrap();
    }
    {
        let mut index = write.open_store("index").unwrap();
        index.set(b"name:record", b"id:1").unwrap();
    }

    assert_eq!(
        before.open_store("primary").unwrap().get(b"id:1").unwrap(),
        None
    );
    assert_eq!(
        before
            .open_store("index")
            .unwrap()
            .get(b"name:record")
            .unwrap(),
        None
    );

    write.commit().unwrap();

    assert_eq!(
        get(&db, "atomic", "primary", b"id:1"),
        Some(b"record".to_vec())
    );
    assert_eq!(
        get(&db, "atomic", "index", b"name:record"),
        Some(b"id:1".to_vec())
    );
}

pub(crate) fn scans<DB: MultiStore>(make_db: &impl Fn() -> DB) {
    let db = make_db();
    db.prepare("scans", ["items"]).unwrap();
    commit_entries(&db, "scans", "items", &scan_entries());

    let cases = vec![
        ScanCase::new(
            "full scan left-to-right",
            (Bound::Unbounded, Bound::Unbounded),
            Direction::LeftToRight,
            &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        ),
        ScanCase::new(
            "full scan right-to-left",
            (Bound::Unbounded, Bound::Unbounded),
            Direction::RightToLeft,
            &[11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0],
        ),
        ScanCase::new(
            "inclusive lower bound",
            (Bound::Included(v(&[1])), Bound::Unbounded),
            Direction::LeftToRight,
            &[4, 5, 6, 7, 8, 9, 10, 11],
        ),
        ScanCase::new(
            "exclusive lower bound",
            (Bound::Excluded(v(&[1])), Bound::Unbounded),
            Direction::LeftToRight,
            &[5, 6, 7, 8, 9, 10, 11],
        ),
        ScanCase::new(
            "inclusive upper bound",
            (Bound::Unbounded, Bound::Included(v(&[1]))),
            Direction::LeftToRight,
            &[0, 1, 2, 3, 4],
        ),
        ScanCase::new(
            "exclusive upper bound",
            (Bound::Unbounded, Bound::Excluded(v(&[1]))),
            Direction::LeftToRight,
            &[0, 1, 2, 3],
        ),
        ScanCase::new(
            "bounded inclusive/inclusive",
            (Bound::Included(v(&[0, 1])), Bound::Included(v(&[2]))),
            Direction::LeftToRight,
            &[3, 4, 5, 6],
        ),
        ScanCase::new(
            "bounded inclusive/exclusive",
            (Bound::Included(v(&[0, 1])), Bound::Excluded(v(&[2]))),
            Direction::LeftToRight,
            &[3, 4, 5],
        ),
        ScanCase::new(
            "bounded exclusive/inclusive",
            (Bound::Excluded(v(&[0, 1])), Bound::Included(v(&[2]))),
            Direction::LeftToRight,
            &[4, 5, 6],
        ),
        ScanCase::new(
            "bounded exclusive/exclusive",
            (Bound::Excluded(v(&[0, 1])), Bound::Excluded(v(&[2]))),
            Direction::LeftToRight,
            &[4, 5],
        ),
        ScanCase::new(
            "empty range",
            (Bound::Included(v(&[4])), Bound::Included(v(&[5]))),
            Direction::LeftToRight,
            &[],
        ),
        ScanCase::new(
            "single-item range",
            (Bound::Included(v(&[1, 0])), Bound::Included(v(&[1, 0]))),
            Direction::LeftToRight,
            &[5],
        ),
        ScanCase::new(
            "reverse scan with bounds",
            (Bound::Included(v(&[0, 1])), Bound::Included(v(&[2]))),
            Direction::RightToLeft,
            &[6, 5, 4, 3],
        ),
        ScanCase::new(
            "empty prefix left-to-right",
            encoded_prefix_range(v(&[])),
            Direction::LeftToRight,
            &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        ),
        ScanCase::new(
            "empty prefix right-to-left",
            encoded_prefix_range(v(&[])),
            Direction::RightToLeft,
            &[11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0],
        ),
        ScanCase::new(
            "prefix with exact key and descendants",
            encoded_prefix_range(v(&[0])),
            Direction::LeftToRight,
            &[1, 2, 3],
        ),
        ScanCase::new(
            "reverse prefix with exact key and descendants",
            encoded_prefix_range(v(&[1])),
            Direction::RightToLeft,
            &[5, 4],
        ),
        ScanCase::new(
            "prefix with descendants but no exact key",
            encoded_prefix_range(v(&[3])),
            Direction::LeftToRight,
            &[7, 8],
        ),
        ScanCase::new(
            "reverse prefix with descendants but no exact key",
            encoded_prefix_range(v(&[3])),
            Direction::RightToLeft,
            &[8, 7],
        ),
        ScanCase::new(
            "single-item prefix",
            encoded_prefix_range(v(&[1, 0])),
            Direction::LeftToRight,
            &[5],
        ),
        ScanCase::new(
            "prefix without finite upper bound",
            encoded_prefix_range(v(&[255])),
            Direction::LeftToRight,
            &[10, 11],
        ),
        ScanCase::new(
            "reverse prefix without finite upper bound",
            encoded_prefix_range(v(&[255])),
            Direction::RightToLeft,
            &[11, 10],
        ),
        ScanCase::new(
            "missing prefix",
            encoded_prefix_range(v(&[4])),
            Direction::LeftToRight,
            &[],
        ),
        ScanCase::new(
            "lexicographic byte ordering",
            (Bound::Included(v(&[2])), Bound::Included(v(&[10]))),
            Direction::LeftToRight,
            &[6, 7, 8, 9],
        ),
    ];

    for case in cases {
        case.assert(&db);
    }

    remove_and_commit(&db, "scans", "items", &[1, 0]);

    for case in [ScanCase::new(
        "scan after remove",
        (Bound::Unbounded, Bound::Unbounded),
        Direction::LeftToRight,
        &[0, 1, 2, 3, 4, 6, 7, 8, 9, 10, 11],
    )] {
        case.assert(&db);
    }
}

fn commit_entries<DB: MultiStore>(
    db: &DB,
    namespace: &'static str,
    store: &'static str,
    entries: &[(Vec<u8>, Vec<u8>)],
) {
    let mut write = db.write(namespace).unwrap();
    {
        let mut store = write.open_store(store).unwrap();
        for (key, value) in entries {
            store.set(key, value).unwrap();
        }
    }
    write.commit().unwrap();
}

fn remove_and_commit<DB: MultiStore>(
    db: &DB,
    namespace: &'static str,
    store: &'static str,
    key: impl AsRef<[u8]>,
) {
    let mut write = db.write(namespace).unwrap();
    {
        let mut store = write.open_store(store).unwrap();
        store.remove(key).unwrap();
    }
    write.commit().unwrap();
}

fn get<DB: MultiStore>(
    db: &DB,
    namespace: &'static str,
    store: &'static str,
    key: impl AsRef<[u8]>,
) -> Option<Vec<u8>> {
    db.read(namespace)
        .unwrap()
        .open_store(store)
        .unwrap()
        .get(key)
        .unwrap()
}

struct ScanCase {
    name: &'static str,
    left: Bound<Vec<u8>>,
    right: Bound<Vec<u8>>,
    direction: Direction,
    expected_indexes: &'static [usize],
}

impl ScanCase {
    fn new(
        name: &'static str,
        range: (Bound<Vec<u8>>, Bound<Vec<u8>>),
        direction: Direction,
        expected_indexes: &'static [usize],
    ) -> Self {
        let (left, right) = range;
        Self {
            name,
            left,
            right,
            direction,
            expected_indexes,
        }
    }

    fn assert<DB: MultiStore>(self, db: &DB) {
        let expected = self
            .expected_indexes
            .into_iter()
            .map(|index| scan_entries()[*index].clone())
            .collect::<Vec<_>>();

        assert_eq!(
            scan(
                db,
                "scans",
                "items",
                (self.left, self.right),
                self.direction
            ),
            expected,
            "{}",
            self.name,
        );
    }
}

fn scan<DB: MultiStore>(
    db: &DB,
    namespace: &'static str,
    store: &'static str,
    range: impl RangeBounds<Vec<u8>>,
    direction: Direction,
) -> Vec<(Vec<u8>, Vec<u8>)> {
    db.read(namespace)
        .unwrap()
        .open_store(store)
        .unwrap()
        .scan(range, direction)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn scan_entries() -> Vec<(Vec<u8>, Vec<u8>)> {
    vec![
        (v(&[]), b"empty".to_vec()),
        (v(&[0]), b"zero".to_vec()),
        (v(&[0, 0]), b"zero-zero".to_vec()),
        (v(&[0, 1]), b"zero-one".to_vec()),
        (v(&[1]), b"one".to_vec()),
        (v(&[1, 0]), b"one-zero".to_vec()),
        (v(&[2]), b"two".to_vec()),
        (v(&[3, 0]), b"three-zero".to_vec()),
        (v(&[3, 1]), b"three-one".to_vec()),
        (v(&[10]), b"ten".to_vec()),
        (v(&[255]), b"max".to_vec()),
        (v(&[255, 0]), b"max-zero".to_vec()),
    ]
}

fn v(bytes: &[u8]) -> Vec<u8> {
    bytes.to_vec()
}
