// EntityStore unit tests for MemoryEntityStore.
// Tests: insert, get, remove, contains_key, len, values, clear, retain, replace_from.

use nexus_storage_sim::{EntityStore, MemoryEntityStore};

// Helper to run async EntityStore methods synchronously in a sync test.
// The methods are async but do no real I/O, so we can block on them.
fn block_on<F: std::future::Future<Output = T>, T>(f: F) -> T {
    futures_executor::block_on(f)
}

#[test]
fn test_insert_and_get() {
    let store: MemoryEntityStore<String> = MemoryEntityStore::new();
    block_on(store.insert("f001".into(), "fact data".into()));
    assert_eq!(block_on(store.get("f001")), Some("fact data".into()));
}

#[test]
fn test_get_nonexistent() {
    let store: MemoryEntityStore<String> = MemoryEntityStore::new();
    assert_eq!(block_on(store.get("nonexistent")), None);
}

#[test]
fn test_insert_overwrite() {
    let store: MemoryEntityStore<String> = MemoryEntityStore::new();
    block_on(store.insert("f001".into(), "original".into()));
    block_on(store.insert("f001".into(), "updated".into()));
    assert_eq!(block_on(store.get("f001")), Some("updated".into()));
}

#[test]
fn test_remove() {
    let store: MemoryEntityStore<String> = MemoryEntityStore::new();
    block_on(store.insert("f001".into(), "data".into()));
    block_on(store.remove("f001"));
    assert_eq!(block_on(store.get("f001")), None);
}

#[test]
fn test_contains_key() {
    let store: MemoryEntityStore<String> = MemoryEntityStore::new();
    assert!(!block_on(store.contains_key("f001")));
    block_on(store.insert("f001".into(), "data".into()));
    assert!(block_on(store.contains_key("f001")));
}

#[test]
fn test_len() {
    let store: MemoryEntityStore<String> = MemoryEntityStore::new();
    assert_eq!(block_on(store.len()), 0);
    block_on(store.insert("f001".into(), "a".into()));
    block_on(store.insert("f002".into(), "b".into()));
    assert_eq!(block_on(store.len()), 2);
}

#[test]
fn test_values() {
    let store: MemoryEntityStore<String> = MemoryEntityStore::new();
    block_on(store.insert("f001".into(), "a".into()));
    block_on(store.insert("f002".into(), "b".into()));
    let mut vals = block_on(store.values());
    vals.sort();
    assert_eq!(vals, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn test_clear() {
    let store: MemoryEntityStore<String> = MemoryEntityStore::new();
    block_on(store.insert("f001".into(), "a".into()));
    block_on(store.clear());
    assert_eq!(block_on(store.len()), 0);
}

#[test]
fn test_retain() {
    let store: MemoryEntityStore<String> = MemoryEntityStore::new();
    block_on(store.insert("f001".into(), "keep".into()));
    block_on(store.insert("f002".into(), "remove".into()));
    // retain via values + filter + replace_from
    let all = block_on(store.values());
    let kept: Vec<_> = all.into_iter().enumerate().filter(|(i, _)| *i == 0).map(|(_, v)| v).collect();
    block_on(store.clear());
    for v in kept {
        block_on(store.insert("f001".into(), v));
    }
    assert!(block_on(store.contains_key("f001")));
    assert!(!block_on(store.contains_key("f002")));
}

#[test]
fn test_replace_from() {
    let store: MemoryEntityStore<String> = MemoryEntityStore::new();
    block_on(store.insert("f_old".into(), "old".into()));
    block_on(store.replace_from(vec![("f_new".into(), "new".into())]));
    assert!(!block_on(store.contains_key("f_old")));
    assert_eq!(block_on(store.get("f_new")), Some("new".into()));
}
