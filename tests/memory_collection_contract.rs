mod common;

use colette::backend::memory::InMemoryMultiStore;

fn make_db() -> InMemoryMultiStore {
    InMemoryMultiStore::new()
}

collection_contract_tests!(make_db);
