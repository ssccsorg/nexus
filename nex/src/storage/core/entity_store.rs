// ── EntityStore: replaceable HashMap backend for FihStorage caches ──────
//
// Single-threaded by design. `FihStorage` is NOT Send+Sync — it belongs
// to one thread (or one WASM isolate). Use `RefCell` for interior
// mutability instead of `RwLock`/`Mutex`, which are not supported on
// `wasm32-unknown-unknown`.
//
// For multi-threaded servers, wrap in `Arc<Mutex<dyn EntityStore>>`
// externally or implement a dedicated `ThreadSafeEntityStore` trait.

use std::cell::RefCell;
use std::collections::HashMap;

/// Type alias for retain predicate. `Send` removed — single-threaded.
/// Returns `true` to keep the entry (same semantics as `HashMap::retain`).
pub(crate) type RetainPredicate<V> = Box<dyn FnMut(&str, &mut V) -> bool>;

// ── EntityStore trait ────────────────────────────────────────────────────

/// A replaceable key-value store for FIH records.
///
/// NOT Send+Sync. Single-threaded use only. Each FihStorage instance
/// is used by one thread (or one WASM isolate) at a time.
///
/// Methods return owned values to avoid lifetime coupling with RefCell.
pub trait EntityStore<V>
where
    V: Clone + 'static,
{
    fn get(&self, key: &str) -> Option<V>;
    fn insert(&self, key: String, value: V) -> Option<V>;
    fn remove(&self, key: &str) -> Option<V>;
    fn contains_key(&self, key: &str) -> bool;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn values(&self) -> Vec<V>;
    fn clear(&self);
    fn retain(&self, f: RetainPredicate<V>);
    fn replace_from(&self, entries: Vec<(String, V)>);
}

// ── MemoryEntityStore ────────────────────────────────────────────────────

/// In-memory EntityStore backed by `RefCell<HashMap<String, V>>`.
///
/// Single-threaded. Safe on WASM (no std::sync::Mutex/RwLock panic).
pub struct MemoryEntityStore<V> {
    inner: RefCell<HashMap<String, V>>,
}

impl<V> MemoryEntityStore<V>
where
    V: Clone + 'static,
{
    pub fn new() -> Self {
        Self {
            inner: RefCell::new(HashMap::new()),
        }
    }
}

impl<V> Default for MemoryEntityStore<V>
where
    V: Clone + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<V> EntityStore<V> for MemoryEntityStore<V>
where
    V: Clone + 'static,
{
    fn get(&self, key: &str) -> Option<V> {
        self.inner.borrow().get(key).cloned()
    }

    fn insert(&self, key: String, value: V) -> Option<V> {
        self.inner.borrow_mut().insert(key, value)
    }

    fn remove(&self, key: &str) -> Option<V> {
        self.inner.borrow_mut().remove(key)
    }

    fn contains_key(&self, key: &str) -> bool {
        self.inner.borrow().contains_key(key)
    }

    fn len(&self) -> usize {
        self.inner.borrow().len()
    }

    fn values(&self) -> Vec<V> {
        self.inner.borrow().values().cloned().collect()
    }

    fn clear(&self) {
        self.inner.borrow_mut().clear();
    }

    fn retain(&self, mut f: RetainPredicate<V>) {
        self.inner.borrow_mut().retain(|k, v| f(k.as_str(), v));
    }

    fn replace_from(&self, entries: Vec<(String, V)>) {
        let mut map = self.inner.borrow_mut();
        map.clear();
        map.extend(entries);
    }
}
