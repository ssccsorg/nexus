// ── EntityStore: replaceable HashMap backend for FihStorage caches ──────

use std::collections::HashMap;

use async_trait::async_trait;
use tagma_core::{Coord, CoordPath, CoordSpaceN};

use crate::core::index::Cell2;

// ── EntityStore trait ────────────────────────────────────────────────────

/// EntityStore: replaceable key-value store for FIH records.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
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

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[async_trait(?Send)]
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

// ── CoordEntityStore: CoordSpaceN-backed EntityStore ──────────────────

/// Map a string key to a CoordPath<N> deterministically without hashing.
///
/// Supports two formats:
/// - 6-char Hangul: each character is a direct Coord (Phase 2+, CoordId format)
/// - Other (64-char hex, etc.): byte decomposition (Phase 1 backward compat)
fn str_to_coordpath<const N: usize>(key: &str) -> CoordPath<N> {
    let chars: Vec<char> = key.chars().collect();
    // Fast path: N-character Hangul key → direct Coord mapping
    if chars.len() == N && chars.iter().all(|c| Coord::from_char(*c).is_some()) {
        let mut coords = [Coord::new(0).unwrap(); N];
        for (i, &ch) in chars.iter().enumerate() {
            coords[i] = Coord::from_char(ch).unwrap();
        }
        return CoordPath::new(coords);
    }
    // Fallback: byte decomposition (backward compat with hex keys)
    let bytes = key.as_bytes();
    let mut coords = [Coord::new(0).unwrap(); N];
    for (i, coord) in coords.iter_mut().enumerate() {
        let hi = bytes.get(i * 2).copied().unwrap_or(0) as u16;
        let lo = bytes.get(i * 2 + 1).copied().unwrap_or(0) as u16;
        let idx = (hi << 8 | lo) % 11172;
        *coord = Coord::new(idx).unwrap();
    }
    CoordPath::new(coords)
}

/// Backward-compatible String-to-String mapping: convert hex key to
/// CoordPath display string (used for `values()` / `replace_from()`).
#[allow(dead_code)]
fn coordpath_to_str<const N: usize>(path: &CoordPath<N>) -> String {
    let mut s = String::with_capacity(N * 3);
    for c in path.coords() {
        s.push(c.to_char());
    }
    s
}

/// EntityStore backed by CoordSpaceN instead of HashMap.
///
/// String keys are mapped to CoordPath<N> deterministically.
/// This is the bridge between the current FihHash-hex-keyed storage
/// interface and the future CoordPath-native storage.
pub struct CoordEntityStore<const N: usize, V> {
    inner: Cell2<CoordSpaceN<N, V>>,
}

impl<const N: usize, V> CoordEntityStore<N, V>
where
    V: Clone + 'static,
{
    pub fn new() -> Self {
        Self {
            inner: Cell2::new(CoordSpaceN::new()),
        }
    }
}

impl<const N: usize, V> Default for CoordEntityStore<N, V>
where
    V: Clone + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
#[async_trait]
impl<const N: usize, V> EntityStore<V> for CoordEntityStore<N, V>
where
    V: Clone + Send + 'static,
{
    async fn get(&self, key: &str) -> Option<V> {
        let path = str_to_coordpath::<N>(key);
        self.inner.borrow().at_path(&path).cloned()
    }

    async fn insert(&self, key: String, value: V) -> Option<V> {
        let path = str_to_coordpath::<N>(&key);
        self.inner.borrow_mut().place_path(&path, value)
    }

    async fn remove(&self, key: &str) -> Option<V> {
        let path = str_to_coordpath::<N>(key);
        self.inner.borrow_mut().vacate_path(&path)
    }

    async fn contains_key(&self, key: &str) -> bool {
        let path = str_to_coordpath::<N>(key);
        self.inner.borrow().at_path(&path).is_some()
    }

    async fn len(&self) -> usize {
        self.inner.borrow().len()
    }

    async fn values(&self) -> Vec<V> {
        let space = self.inner.borrow();
        let mut values = Vec::with_capacity(space.len());
        for (_path, v) in space.iter_tree() {
            values.push(v.clone());
        }
        values
    }

    async fn clear(&self) {
        self.inner.borrow_mut().clear();
    }

    async fn replace_from(&self, entries: Vec<(String, V)>) {
        let mut space = self.inner.borrow_mut();
        space.clear();
        for (key, value) in entries {
            let path = str_to_coordpath::<N>(&key);
            space.place_path(&path, value);
        }
    }
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[async_trait(?Send)]
impl<const N: usize, V> EntityStore<V> for CoordEntityStore<N, V>
where
    V: Clone + 'static,
{
    async fn get(&self, key: &str) -> Option<V> {
        let path = str_to_coordpath::<N>(key);
        self.inner.borrow().at_path(&path).cloned()
    }

    async fn insert(&self, key: String, value: V) -> Option<V> {
        let path = str_to_coordpath::<N>(&key);
        self.inner.borrow_mut().place_path(&path, value)
    }

    async fn remove(&self, key: &str) -> Option<V> {
        let path = str_to_coordpath::<N>(key);
        self.inner.borrow_mut().vacate_path(&path)
    }

    async fn contains_key(&self, key: &str) -> bool {
        let path = str_to_coordpath::<N>(key);
        self.inner.borrow().at_path(&path).is_some()
    }

    async fn len(&self) -> usize {
        self.inner.borrow().len()
    }

    async fn values(&self) -> Vec<V> {
        let space = self.inner.borrow();
        let mut values = Vec::with_capacity(space.len());
        for (_path, v) in space.iter_tree() {
            values.push(v.clone());
        }
        values
    }

    async fn clear(&self) {
        self.inner.borrow_mut().clear();
    }

    async fn replace_from(&self, entries: Vec<(String, V)>) {
        let mut space = self.inner.borrow_mut();
        space.clear();
        for (key, value) in entries {
            let path = str_to_coordpath::<N>(&key);
            space.place_path(&path, value);
        }
    }
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

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
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

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
#[async_trait(?Send)]
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
