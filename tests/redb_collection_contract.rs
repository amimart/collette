#![cfg(feature = "redb")]

mod common;

use collette::backend::redb::RedbMultiStore;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DB: AtomicU64 = AtomicU64::new(0);

fn make_db() -> RedbMultiStore {
    RedbMultiStore::create(temp_db_path()).unwrap()
}

fn temp_db_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "collette-redb-collection-{}-{}.redb",
        std::process::id(),
        NEXT_DB.fetch_add(1, Ordering::Relaxed)
    ))
}

collection_contract_tests!(make_db);
