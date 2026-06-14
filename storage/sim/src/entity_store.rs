// ── EntityStore: replaceable HashMap backend for FihStorage caches ──────
//
// The single abstraction point for replacing FihStorage's in-memory
// HashMap caches (FactCache, IntentCache, HintCache) with alternative
// backends. Every EntityStore method mirrors the HashMap operations
// that FihStorage currently calls directly.
//
// Implementations:
//   - MemoryEntityStore: RwLock<HashMap> — current behavior, always available
//   - TonboEntityStore: (future) Tonbo WAL + Parquet for durability
//
// Design decisions:
//
//   Owned values (not references):
//     The trait returns `Option<V>` rather than `Option<&V>`. This avoids
//     lifetime complexity from borrowing across RwLock boundaries and keeps
//     the trait simple enough for Tonbo (which deals in owned RecordBatches).
//     FihStorage's hot-path reads already clone values from the caches, so
//     the overhead is identical to the current implementation.
//
//   Per-kind instances (not unified):
//     FihStorage uses three caches (FactCache, IntentCache, HintCache) with
//     different value types. A single unified EntityStore<HashMap<String, V>>
//     would lose type safety. Instead, each cache is its own EntityStore
//     instance with the correct value type.
//
//   Sync (not async):
//     Tonbo integration can wrap this with async internally. The trait is
//     deliberately sync to match FihStorage's synchronous trait impls
//     (FactCapable, IntentCapable, etc.).

use std::collections::HashMap;
use std::sync::RwLock;

/// Type alias for retain predicate to suppress clippy::type_complexity.
pub(crate) type RetainPredicate<V> = Box<dyn FnMut(&str, &mut V) -> bool + Send>;

// ── EntityStore trait ────────────────────────────────────────────────────

/// A replaceable key-value store for FIH records.
///
/// Each EntityStore instance manages one record kind with its specific type:
///   - `Box<dyn EntityStore<FactRecord>>`   for facts
///   - `Box<dyn EntityStore<IntentRecord>>` for intents
///   - `Box<dyn EntityStore<HintRecord>>`   for hints
///
/// Methods return owned values to avoid lifetime coupling with internal locks.
pub trait EntityStore<V>: Send + Sync
where
    V: Send + Sync,
{
    /// Returns the value for `key`, or None if missing.
    fn get(&self, key: &str) -> Option<V>;

    /// Insert `value` at `key`. Returns the previous value, if any.
    fn insert(&self, key: String, value: V) -> Option<V>;

    /// Remove `key`. Returns the removed value, if any.
    fn remove(&self, key: &str) -> Option<V>;

    /// Returns true if `key` exists.
    fn contains_key(&self, key: &str) -> bool;

    /// Returns the number of entries.
    fn len(&self) -> usize;

    /// Returns true if the store is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns all values.
    fn values(&self) -> Vec<V>;

    /// Clear all entries.
    fn clear(&self);

    /// Retain only entries matching a predicate.
    fn retain(&self, f: RetainPredicate<V>);

    /// Replace all contents. Atomically clears and inserts from a Vec.
    fn replace_from(&self, entries: Vec<(String, V)>);
}

// ── MemoryEntityStore ────────────────────────────────────────────────────

/// In-memory EntityStore backed by `RwLock<HashMap<String, V>>`.
///
/// Exactly mirrors FihStorage's current cache behavior. Thread-safe.
pub struct MemoryEntityStore<V> {
    inner: RwLock<HashMap<String, V>>,
}

impl<V> MemoryEntityStore<V>
where
    V: Clone + Send + Sync + 'static,
{
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
    }
}

impl<V> Default for MemoryEntityStore<V>
where
    V: Clone + Send + Sync + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<V> EntityStore<V> for MemoryEntityStore<V>
where
    V: Clone + Send + Sync + 'static,
{
    fn get(&self, key: &str) -> Option<V> {
        self.inner.read().unwrap().get(key).cloned()
    }

    fn insert(&self, key: String, value: V) -> Option<V> {
        self.inner.write().unwrap().insert(key, value)
    }

    fn remove(&self, key: &str) -> Option<V> {
        self.inner.write().unwrap().remove(key)
    }

    fn contains_key(&self, key: &str) -> bool {
        self.inner.read().unwrap().contains_key(key)
    }

    fn len(&self) -> usize {
        self.inner.read().unwrap().len()
    }

    fn values(&self) -> Vec<V> {
        self.inner.read().unwrap().values().cloned().collect()
    }

    fn clear(&self) {
        self.inner.write().unwrap().clear();
    }

    fn retain(&self, mut f: RetainPredicate<V>) {
        self.inner.write().unwrap().retain(|k, v| f(k.as_str(), v));
    }

    fn replace_from(&self, entries: Vec<(String, V)>) {
        let mut map = self.inner.write().unwrap();
        map.clear();
        map.extend(entries);
    }
}
