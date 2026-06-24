// ── EntityStore: replaceable HashMap backend for FihStorage caches ──────

use std::collections::HashMap;

use crate::storage::core::index::Cell2;

/// Retain predicate type. Not Send on wasm, Send on native.
pub(crate) type RetainPredicate<V> = Box<dyn FnMut(&str, &mut V) -> bool>;

// ── EntityStore trait ────────────────────────────────────────────────────

/// EntityStore: replaceable key-value store for FIH records.
#[cfg(not(target_arch = "wasm32"))]
pub trait EntityStore<V>: Send + Sync
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

#[cfg(target_arch = "wasm32")]
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

/// In-memory EntityStore using Cell2 (Mutex on native, RefCell on wasm).
pub struct MemoryEntityStore<V> {
    inner: Cell2<HashMap<String, V>>,
}

impl<V> MemoryEntityStore<V>
where
    V: Clone + 'static,
{
    pub fn new() -> Self {
        Self {
            inner: Cell2::new(HashMap::new()),
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

#[cfg(not(target_arch = "wasm32"))]
impl<V> EntityStore<V> for MemoryEntityStore<V>
where
    V: Clone + Send + 'static,
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

#[cfg(target_arch = "wasm32")]
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
