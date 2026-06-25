// ── EntityStore: replaceable HashMap backend for FihStorage caches ──────

use std::collections::HashMap;

use async_trait::async_trait;

use crate::storage::core::index::Cell2;

// ── EntityStore trait ────────────────────────────────────────────────────

/// EntityStore: replaceable key-value store for FIH records.
#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
pub trait EntityStore<V>: Send + Sync
where
    V: Clone + 'static,
{
    async fn get(&self, key: &str) -> Option<V>;
    async fn insert(&self, key: String, value: V) -> Option<V>;
    async fn remove(&self, key: &str) -> Option<V>;
    async fn contains_key(&self, key: &str) -> bool;
    async fn len(&self) -> usize;
    async fn is_empty(&self) -> bool {
        self.len().await == 0
    }
    async fn values(&self) -> Vec<V>;
    async fn clear(&self);
    async fn replace_from(&self, entries: Vec<(String, V)>);
}

#[cfg(target_arch = "wasm32")]
#[async_trait]
pub trait EntityStore<V>
where
    V: Clone + 'static,
{
    async fn get(&self, key: &str) -> Option<V>;
    async fn insert(&self, key: String, value: V) -> Option<V>;
    async fn remove(&self, key: &str) -> Option<V>;
    async fn contains_key(&self, key: &str) -> bool;
    async fn len(&self) -> usize;
    async fn is_empty(&self) -> bool {
        self.len().await == 0
    }
    async fn values(&self) -> Vec<V>;
    async fn clear(&self);
    async fn replace_from(&self, entries: Vec<(String, V)>);
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

    /// Sync accessor for use in sync trait impls (RecordLoad, FihRecordLoad)
    /// that cannot use async EntityStore methods.
    pub fn get_sync(&self, key: &str) -> Option<V> {
        self.inner.borrow().get(key).cloned()
    }

    /// Sync values accessor for use in sync trait impls.
    pub fn values_sync(&self) -> Vec<V> {
        self.inner.borrow().values().cloned().collect()
    }

    /// Sync len accessor for use in sync trait impls.
    pub fn len_sync(&self) -> usize {
        self.inner.borrow().len()
    }

    /// Sync is_empty accessor for use in sync trait impls.
    pub fn is_empty_sync(&self) -> bool {
        self.inner.borrow().is_empty()
    }

    /// Sync contains_key accessor for use in sync trait impls.
    pub fn contains_key_sync(&self, key: &str) -> bool {
        self.inner.borrow().contains_key(key)
    }

    /// Sync retain for use in contexts where async is not possible.
    /// Since retain only does pure computation, it does not need to be async.
    pub fn retain_sync(&self, mut f: Box<dyn FnMut(&str, &mut V) -> bool + Send>) {
        let mut map = self.inner.borrow_mut();
        map.retain(|k, v| f(k.as_str(), v));
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
#[async_trait]
impl<V> EntityStore<V> for MemoryEntityStore<V>
where
    V: Clone + Send + 'static,
{
    async fn get(&self, key: &str) -> Option<V> {
        self.inner.borrow().get(key).cloned()
    }

    async fn insert(&self, key: String, value: V) -> Option<V> {
        self.inner.borrow_mut().insert(key, value)
    }

    async fn remove(&self, key: &str) -> Option<V> {
        self.inner.borrow_mut().remove(key)
    }

    async fn contains_key(&self, key: &str) -> bool {
        self.inner.borrow().contains_key(key)
    }

    async fn len(&self) -> usize {
        self.inner.borrow().len()
    }

    async fn values(&self) -> Vec<V> {
        self.inner.borrow().values().cloned().collect()
    }

    async fn clear(&self) {
        self.inner.borrow_mut().clear();
    }

    async fn replace_from(&self, entries: Vec<(String, V)>) {
        let mut map = self.inner.borrow_mut();
        map.clear();
        map.extend(entries);
    }
}

#[cfg(target_arch = "wasm32")]
#[async_trait]
impl<V> EntityStore<V> for MemoryEntityStore<V>
where
    V: Clone + 'static,
{
    async fn get(&self, key: &str) -> Option<V> {
        self.inner.borrow().get(key).cloned()
    }

    async fn insert(&self, key: String, value: V) -> Option<V> {
        self.inner.borrow_mut().insert(key, value)
    }

    async fn remove(&self, key: &str) -> Option<V> {
        self.inner.borrow_mut().remove(key)
    }

    async fn contains_key(&self, key: &str) -> bool {
        self.inner.borrow().contains_key(key)
    }

    async fn len(&self) -> usize {
        self.inner.borrow().len()
    }

    async fn values(&self) -> Vec<V> {
        self.inner.borrow().values().cloned().collect()
    }

    async fn clear(&self) {
        self.inner.borrow_mut().clear();
    }

    async fn replace_from(&self, entries: Vec<(String, V)>) {
        let mut map = self.inner.borrow_mut();
        map.clear();
        map.extend(entries);
    }
}
