pub use tagma_core::{Coord, CoordKey, CoordKeyMap};

/// Convenience: check whether a Unicode code point is a valid Tagma coordinate.
pub fn validate(cp: u16) -> bool {
    Coord::from_code_point(cp).is_some()
}

/// In-memory key-value store backed by Tagma's hash-free direct addressing.
/// Proxy pattern: mirrors nexus `EntityStore` interface for validation
/// before the implementation moves into nexus core.
pub struct MemStore<V> {
    inner: std::cell::RefCell<CoordKeyMap<1, String, V>>,
}

impl<V> MemStore<V> {
    pub fn new() -> Self {
        MemStore {
            inner: std::cell::RefCell::new(CoordKeyMap::new()),
        }
    }

    pub fn get(&self, key: &str) -> Option<V>
    where
        V: Clone,
    {
        self.inner.borrow().get_str(key).cloned()
    }

    pub fn insert(&self, key: String, value: V) -> Option<V> {
        self.inner.borrow_mut().insert(key, value)
    }

    pub fn remove(&self, key: &str) -> Option<V> {
        self.inner.borrow_mut().remove_str(key)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.inner.borrow().get_str(key).is_some()
    }

    pub fn len(&self) -> usize {
        self.inner.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.borrow().is_empty()
    }

    pub fn values(&self) -> Vec<V>
    where
        V: Clone,
    {
        self.inner.borrow().values()
    }

    pub fn clear(&self) {
        self.inner.borrow_mut().clear();
    }

    pub fn replace_from(&self, entries: Vec<(String, V)>) {
        let mut map = self.inner.borrow_mut();
        map.clear();
        for (k, v) in entries {
            map.insert(k, v);
        }
    }
}
