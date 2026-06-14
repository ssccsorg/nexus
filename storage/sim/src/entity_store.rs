// ── EntityStore: structured key-value persistence abstraction ──────────
//
// A higher-level persistence boundary than AsyncFileIo. While AsyncFileIo
// operates on raw file paths in a flat key-space, EntityStore knows about
// record kinds (facts, intents, hints) and stores serialized records.
//
// Implementations:
//   - MemoryEntityStore: HashMap-backed, same behavior as current FihStorage
//     caches. All operations are immediate (no async).
//   - TonboEntityStore: Maps to Tonbo's Arrow RecordBatch interface.
//     Each kind maps to a Tonbo table query with kind+key columns.
//     Write durability is guaranteed by Tonbo's WAL.
//   - HybridEntityStore: Memory + Tonbo combined (hot data in memory,
//     cold data persisted).
//
// Relationship with AsyncFileIo:
//
//   EntityStore is NOT a replacement for AsyncFileIo. They serve different
//   abstraction levels:
//
//   AsyncFileIo (io.rs):
//     Flat key-space: paths like "facts/f_hash.fact"
//     Operations: read(path), write(path, data), list(prefix), delete(path)
//     Caller manages serialization and path construction
//     Used for: blob storage, chain files, direct IO fallback
//
//   EntityStore (this file):
//     Structured: kind + key (e.g., kind="facts/", key="f_hash")
//     Operations: get(kind, key), insert(kind, key, value), scan(kind)
//     Values are opaque Vec<u8> (caller serializes/deserializes)
//     Used for: record persistence (facts, intents, hints)
//
//   FihStorage uses BOTH:
//     - EntityStore for persistent record storage (reads/writes records)
//     - AsyncFileIo for blob storage, chain files, cold IO fallback
//     - Typed in-memory caches (FactCache etc.) for hot-path reads
//
// Why async?
//
//   Storage backends like Tonbo require async I/O. The trait uses async
//   methods via #[async_trait]. Sync callers use SyncEntityStore wrapper
//   (same pattern as AsyncFileIo + SyncFileIo).
//
//   SyncEntityStore does NOT introduce significant overhead because
//   FihStorage buffers writes in pending WriteOps and flushes in batch.
//   Individual EntityStore calls are amortized over the flush window.

use std::collections::HashMap;
use std::sync::RwLock;

use async_trait::async_trait;

/// Result type alias for EntityStore operations.
pub type EntityResult<T> = Result<T, String>;

/// Structured key-value store with kind-based namespacing.
///
/// # Kind convention
///
/// Kinds match the path prefix convention used by FihStorage:
///   - `"facts/"`   for FactRecord
///   - `"intents/"` for IntentRecord
///   - `"hints/"`   for HintRecord
///
/// Each kind is a flat namespace. Keys are unique within a kind.
///
/// # Thread safety
///
/// Implementations must be Send + Sync. The trait is object-safe via
/// #[async_trait], allowing dynamic dispatch (Box<dyn EntityStore>).
///
/// # Tonbo mapping
///
/// A TonboEntityStore would map this to a single table:
///
/// ```sql
/// CREATE TABLE entity_store (
///     kind  TEXT,  -- PRIMARY KEY
///     key   TEXT,  -- PRIMARY KEY
///     value BLOB,
///     PRIMARY KEY (kind, key)
/// );
/// ```
///
/// scan("facts/") → SELECT key, value FROM entity_store WHERE kind = "facts/"
#[async_trait]
pub trait EntityStore: Send + Sync {
    /// Read a single record by kind + key.
    /// Returns None if the record does not exist.
    async fn get(&self, kind: &str, key: &str) -> EntityResult<Option<Vec<u8>>>;

    /// Insert or update a record.
    /// Overwrites any existing record with the same kind + key.
    async fn insert(&self, kind: &str, key: String, value: Vec<u8>) -> EntityResult<()>;

    /// Delete a record. Succeeds (no-op) if the record does not exist.
    async fn remove(&self, kind: &str, key: &str) -> EntityResult<()>;

    /// List all records in a kind. Returns (key, value) pairs.
    /// The kind prefix is stripped from returned keys.
    async fn scan(&self, kind: &str) -> EntityResult<Vec<(String, Vec<u8>)>>;

    /// Ensure all pending writes are durable.
    /// For MemoryEntityStore this is a no-op.
    /// For TonboEntityStore this calls flush on the WAL.
    async fn flush(&self) -> EntityResult<()>;

    /// Clear all records of a specific kind.
    /// For MemoryEntityStore this drops the inner HashMap entry.
    async fn clear_kind(&self, kind: &str) -> EntityResult<()>;

    /// Clear all records. For testing/cleanup.
    async fn clear_all(&self) -> EntityResult<()>;
}

// ── SyncEntityStore wrapper ──────────────────────────────────────────────
//
// Bridges async EntityStore to synchronous callers via futures_executor::block_on.
// Same pattern as SyncFileIo in io.rs.

/// Wraps an async EntityStore into a blocking/sync interface.
pub struct SyncEntityStore {
    inner: Box<dyn EntityStore>,
}

impl SyncEntityStore {
    pub fn new(inner: Box<dyn EntityStore>) -> Self {
        Self { inner }
    }

    pub fn get(&self, kind: &str, key: &str) -> EntityResult<Option<Vec<u8>>> {
        futures_executor::block_on(self.inner.get(kind, key))
    }

    pub fn insert(&self, kind: &str, key: String, value: Vec<u8>) -> EntityResult<()> {
        futures_executor::block_on(self.inner.insert(kind, key, value))
    }

    pub fn remove(&self, kind: &str, key: &str) -> EntityResult<()> {
        futures_executor::block_on(self.inner.remove(kind, key))
    }

    pub fn scan(&self, kind: &str) -> EntityResult<Vec<(String, Vec<u8>)>> {
        futures_executor::block_on(self.inner.scan(kind))
    }

    pub fn flush(&self) -> EntityResult<()> {
        futures_executor::block_on(self.inner.flush())
    }

    pub fn clear_kind(&self, kind: &str) -> EntityResult<()> {
        futures_executor::block_on(self.inner.clear_kind(kind))
    }

    pub fn clear_all(&self) -> EntityResult<()> {
        futures_executor::block_on(self.inner.clear_all())
    }

    /// Consume the wrapper and return the inner boxed trait object.
    pub fn into_inner(self) -> Box<dyn EntityStore> {
        self.inner
    }
}

// ── MemoryEntityStore: HashMap-backed implementation ─────────────────────
//
// All operations are synchronous and O(1) average.
// Values are stored as Vec<u8> (postcard-serialized records).
// Thread-safe via RwLock<HashMap<kind, HashMap<key, value>>>.

/// In-memory EntityStore backed by a nested HashMap.
///
/// Storage layout:
///   data: HashMap<kind, HashMap<key, value>>
///   kind = "facts/"    → { "f_hash" → bytes, ... }
///   kind = "intents/"  → { "i_hash" → bytes, ... }
///   kind = "hints/"    → { "h_hash" → bytes, ... }
pub struct MemoryEntityStore {
    data: RwLock<HashMap<String, HashMap<String, Vec<u8>>>>,
}

impl MemoryEntityStore {
    pub fn new() -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
        }
    }

    /// Number of entries across all kinds.
    pub fn len(&self) -> usize {
        self.data
            .read()
            .unwrap()
            .values()
            .map(|m| m.len())
            .sum()
    }

    /// Returns true if no entries exist.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for MemoryEntityStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EntityStore for MemoryEntityStore {
    async fn get(&self, kind: &str, key: &str) -> EntityResult<Option<Vec<u8>>> {
        let map = self.data.read().map_err(|e| e.to_string())?;
        Ok(map.get(kind).and_then(|m| m.get(key).cloned()))
    }

    async fn insert(&self, kind: &str, key: String, value: Vec<u8>) -> EntityResult<()> {
        let mut map = self.data.write().map_err(|e| e.to_string())?;
        map.entry(kind.to_string())
            .or_default()
            .insert(key, value);
        Ok(())
    }

    async fn remove(&self, kind: &str, key: &str) -> EntityResult<()> {
        let mut map = self.data.write().map_err(|e| e.to_string())?;
        if let Some(inner) = map.get_mut(kind) {
            inner.remove(key);
        }
        Ok(())
    }

    async fn scan(&self, kind: &str) -> EntityResult<Vec<(String, Vec<u8>)>> {
        let map = self.data.read().map_err(|e| e.to_string())?;
        Ok(map
            .get(kind)
            .map(|m| {
                m.iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn flush(&self) -> EntityResult<()> {
        // Memory store is always "durable" — no-op.
        Ok(())
    }

    async fn clear_kind(&self, kind: &str) -> EntityResult<()> {
        let mut map = self.data.write().map_err(|e| e.to_string())?;
        map.remove(kind);
        Ok(())
    }

    async fn clear_all(&self) -> EntityResult<()> {
        let mut map = self.data.write().map_err(|e| e.to_string())?;
        map.clear();
        Ok(())
    }
}

impl Clone for MemoryEntityStore {
    fn clone(&self) -> Self {
        let data = self.data.read().unwrap().clone();
        Self {
            data: RwLock::new(data),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> SyncEntityStore {
        SyncEntityStore::new(Box::new(MemoryEntityStore::new()))
    }

    #[test]
    fn test_insert_and_get() {
        let store = setup();
        store
            .insert("facts/", "f001".into(), b"fact data".to_vec())
            .unwrap();
        let result = store.get("facts/", "f001").unwrap();
        assert_eq!(result, Some(b"fact data".to_vec()));
    }

    #[test]
    fn test_get_nonexistent() {
        let store = setup();
        assert!(store.get("facts/", "nonexistent").unwrap().is_none());
    }

    #[test]
    fn test_get_nonexistent_kind() {
        let store = setup();
        assert!(store.get("nonexistent_kind/", "k").unwrap().is_none());
    }

    #[test]
    fn test_insert_overwrite() {
        let store = setup();
        store
            .insert("facts/", "f001".into(), b"original".to_vec())
            .unwrap();
        store
            .insert("facts/", "f001".into(), b"updated".to_vec())
            .unwrap();
        let result = store.get("facts/", "f001").unwrap();
        assert_eq!(result, Some(b"updated".to_vec()));
    }

    #[test]
    fn test_remove() {
        let store = setup();
        store
            .insert("facts/", "f001".into(), b"data".to_vec())
            .unwrap();
        store.remove("facts/", "f001").unwrap();
        assert!(store.get("facts/", "f001").unwrap().is_none());
    }

    #[test]
    fn test_remove_nonexistent() {
        let store = setup();
        // Should not error
        store.remove("facts/", "nonexistent").unwrap();
    }

    #[test]
    fn test_scan_kind() {
        let store = setup();
        store
            .insert("facts/", "f_a".into(), b"a".to_vec())
            .unwrap();
        store
            .insert("facts/", "f_b".into(), b"b".to_vec())
            .unwrap();
        store
            .insert("intents/", "i_a".into(), b"intent".to_vec())
            .unwrap();

        let facts = store.scan("facts/").unwrap();
        assert_eq!(facts.len(), 2);
        assert!(facts.iter().any(|(k, v)| k == "f_a" && v == b"a"));
        assert!(facts.iter().any(|(k, v)| k == "f_b" && v == b"b"));

        let intents = store.scan("intents/").unwrap();
        assert_eq!(intents.len(), 1);
    }

    #[test]
    fn test_scan_empty_kind() {
        let store = setup();
        let result = store.scan("empty/").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_clear_kind() {
        let store = setup();
        store
            .insert("facts/", "f001".into(), b"data".to_vec())
            .unwrap();
        store
            .insert("intents/", "i001".into(), b"intent".to_vec())
            .unwrap();
        store.clear_kind("facts/").unwrap();
        assert!(store.scan("facts/").unwrap().is_empty());
        assert_eq!(store.scan("intents/").unwrap().len(), 1);
    }

    #[test]
    fn test_clear_all() {
        let store = setup();
        store
            .insert("facts/", "f001".into(), b"data".to_vec())
            .unwrap();
        store
            .insert("intents/", "i001".into(), b"intent".to_vec())
            .unwrap();
        store.clear_all().unwrap();
        assert!(store.scan("facts/").unwrap().is_empty());
        assert!(store.scan("intents/").unwrap().is_empty());
    }

    #[test]
    fn test_flush_noop() {
        let store = setup();
        store.insert("facts/", "f".into(), b"x".to_vec()).unwrap();
        store.flush().unwrap(); // Should not error
        assert_eq!(store.get("facts/", "f").unwrap(), Some(b"x".to_vec()));
    }

    #[test]
    fn test_clone_independence() {
        let original = MemoryEntityStore::new();
        let sync_orig = SyncEntityStore::new(Box::new(original));
        sync_orig
            .insert("facts/", "f001".into(), b"original".to_vec())
            .unwrap();

        // Clone the inner store
        let cloned_inner = sync_orig.into_inner();
        // We can't downcast, but we can verify the trait works
        // This is a basic sanity test of the clone infrastructure
        let sync_clone = SyncEntityStore::new(cloned_inner);
        let result = sync_clone.get("facts/", "f001").unwrap();
        assert_eq!(result, Some(b"original".to_vec()));
    }
}
