#![cfg(feature = "rocksdb")]

mod common;

use collette::backend::rocksdb::RocksDbMultiStore;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DB: AtomicU64 = AtomicU64::new(0);

fn make_db() -> RocksDbMultiStore {
    RocksDbMultiStore::create(temp_db_path()).unwrap()
}

fn temp_db_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "collette-rocksdb-collection-{}-{}",
        std::process::id(),
        NEXT_DB.fetch_add(1, Ordering::Relaxed)
    ))
}

collection_contract_tests!(make_db);
