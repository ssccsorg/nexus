pub use tagma_core::Coord;
pub use tagma_core::CoordSpace;

/// Convenience: check whether a Unicode code point is a valid Tagma coordinate.
pub fn validate(cp: u16) -> bool {
    Coord::from_code_point(cp).is_some()
}

/// In-memory key-value store backed by Tagma's hash-free direct addressing.
/// Keys are Coord values -- the application manages the key space externally.
/// This is the hashless pattern: Tagma provides storage, not key derivation.
pub struct MemStore<V> {
    inner: std::cell::RefCell<CoordSpace<V>>,
}

impl<V> MemStore<V> {
    pub fn new() -> Self {
        MemStore {
            inner: std::cell::RefCell::new(CoordSpace::new()),
        }
    }

    pub fn get(&self, key: Coord) -> Option<V>
    where
        V: Clone,
    {
        self.inner.borrow().at(&key).cloned()
    }

    pub fn insert(&self, key: Coord, value: V) -> Option<V> {
        self.inner.borrow_mut().place(key, value)
    }

    pub fn remove(&self, key: Coord) -> Option<V> {
        self.inner.borrow_mut().vacate(&key)
    }

    pub fn contains_key(&self, key: Coord) -> bool {
        self.inner.borrow().occupied(&key)
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
        self.inner.borrow().values().cloned().collect()
    }

    pub fn clear(&self) {
        self.inner.borrow_mut().clear();
    }
}
