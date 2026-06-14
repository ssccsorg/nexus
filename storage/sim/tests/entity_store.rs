// EntityStore unit tests for MemoryEntityStore.
// Tests: insert, get, remove, contains_key, len, values, clear, retain, replace_from.

use nexus_storage_sim::{EntityStore, MemoryEntityStore};

#[test]
fn test_insert_and_get() {
    let store: MemoryEntityStore<String> = MemoryEntityStore::new();
    store.insert("f001".into(), "fact data".into());
    assert_eq!(store.get("f001"), Some("fact data".into()));
}

#[test]
fn test_get_nonexistent() {
    let store: MemoryEntityStore<String> = MemoryEntityStore::new();
    assert_eq!(store.get("nonexistent"), None);
}

#[test]
fn test_insert_overwrite() {
    let store: MemoryEntityStore<String> = MemoryEntityStore::new();
    store.insert("f001".into(), "original".into());
    store.insert("f001".into(), "updated".into());
    assert_eq!(store.get("f001"), Some("updated".into()));
}

#[test]
fn test_remove() {
    let store: MemoryEntityStore<String> = MemoryEntityStore::new();
    store.insert("f001".into(), "data".into());
    store.remove("f001");
    assert_eq!(store.get("f001"), None);
}

#[test]
fn test_contains_key() {
    let store: MemoryEntityStore<String> = MemoryEntityStore::new();
    assert!(!store.contains_key("f001"));
    store.insert("f001".into(), "data".into());
    assert!(store.contains_key("f001"));
}

#[test]
fn test_len() {
    let store: MemoryEntityStore<String> = MemoryEntityStore::new();
    assert_eq!(store.len(), 0);
    store.insert("f001".into(), "a".into());
    store.insert("f002".into(), "b".into());
    assert_eq!(store.len(), 2);
}

#[test]
fn test_values() {
    let store: MemoryEntityStore<String> = MemoryEntityStore::new();
    store.insert("f001".into(), "a".into());
    store.insert("f002".into(), "b".into());
    let mut vals = store.values();
    vals.sort();
    assert_eq!(vals, vec!["a".to_string(), "b".to_string()]);
}

#[test]
fn test_clear() {
    let store: MemoryEntityStore<String> = MemoryEntityStore::new();
    store.insert("f001".into(), "a".into());
    store.clear();
    assert_eq!(store.len(), 0);
}

#[test]
fn test_retain() {
    let store: MemoryEntityStore<String> = MemoryEntityStore::new();
    store.insert("f001".into(), "keep".into());
    store.insert("f002".into(), "remove".into());
    store.retain(Box::new(|k, _| k != "f002"));
    assert!(store.contains_key("f001"));
    assert!(!store.contains_key("f002"));
}

#[test]
fn test_replace_from() {
    let store: MemoryEntityStore<String> = MemoryEntityStore::new();
    store.insert("f_old".into(), "old".into());
    store.replace_from(vec![("f_new".into(), "new".into())]);
    assert!(!store.contains_key("f_old"));
    assert_eq!(store.get("f_new"), Some("new".into()));
}
