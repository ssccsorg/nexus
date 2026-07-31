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

    /// Iterate over values matching a predicate, cloning only on match.
    /// Avoids the `values()` → Vec → filter pipeline.
    pub async fn iter_filtered<F>(&self, mut predicate: F) -> Vec<V>
    where
        V: Send,
        F: FnMut(&V) -> bool + Send,
    {
        let space = self.inner.borrow();
        let mut results = Vec::with_capacity(space.len().min(128));
        for (_path, v) in space.iter_tree() {
            if predicate(v) {
                results.push(v.clone());
            }
        }
        results
    }

    /// Filter during tree traversal using path coordinates.
    /// For each entry, checks if `path[axis] == value` for all specified
    /// (axis, value) pairs BEFORE cloning. Avoids string comparison
    /// when the filter corresponds to known axis indices.
    ///
    /// Falls back to full scan (path coord check is still faster than
    /// string compare), but when axes 0..k are fully specified, uses
    /// `iter_prefix` for O(subtree) traversal.
    pub async fn axis_filtered(&self, axis_checks: &[(usize, u16)]) -> Vec<V>
    where
        V: Send,
    {
        let space = self.inner.borrow();

        // If axis_checks cover axes 0..k contiguously from the start,
        // use iter_prefix for the subtree.
        let contiguous_prefix = {
            let mut prefix_len = 0;
            for (i, &(axis, _val)) in axis_checks.iter().enumerate() {
                if axis == i {
                    prefix_len = i + 1;
                } else {
                    break;
                }
            }
            if prefix_len > 0 && prefix_len == axis_checks.len() {
                Some(prefix_len)
            } else {
                None
            }
        };

        if let Some(prefix_len) = contiguous_prefix {
            // Build prefix from the first prefix_len axis values
            let mut prefix_coords = Vec::with_capacity(prefix_len);
            for (_, val) in axis_checks.iter().take(prefix_len) {
                if let Some(c) = tagma_core::Coord::new(*val) {
                    prefix_coords.push(c);
                } else {
                    return Vec::new();
                }
            }
            if let Some(iter) = space.iter_prefix(&prefix_coords) {
                let mut results = Vec::new();
                for (_path, v) in iter {
                    results.push(v.clone());
                }
                return results;
            }
            return Vec::new();
        }

        // Non-contiguous: full scan with path coord check
        let mut results = Vec::new();
        'outer: for (path, v) in space.iter_tree() {
            for &(axis, val) in axis_checks {
                if axis >= N || path.coords()[axis].index() != val {
                    continue 'outer;
                }
            }
            results.push(v.clone());
        }
        results
    }

    /// Iterate over values under a CoordPath prefix, cloning only matching entries.
    /// This is the axis-aware fast path — skips entire subtrees that don't match.
    /// Returns `None` if the prefix path doesn't exist.
    pub async fn iter_prefix_filtered<F>(
        &self,
        prefix: &[tagma_core::Coord],
        mut predicate: F,
    ) -> Option<Vec<V>>
    where
        V: Send,
        F: FnMut(&V) -> bool + Send,
    {
        let space = self.inner.borrow();
        let iter = space.iter_prefix(prefix)?;
        let mut results = Vec::new();
        for (_path, v) in iter {
            if predicate(v) {
                results.push(v.clone());
            }
        }
        Some(results)
    }

    /// Query by axis hints: build a contiguous prefix from provided axis values
    /// and use iter_prefix for O(subtree) traversal. Returns all values under the
    /// prefix, or None if the prefix doesn't exist.
    ///
    /// Axis convention: [0]=time_hi, [1]=time_lo, [2]=entity, [3]=origin, [4]=creator, [5]=serial
    pub async fn query_prefix(&self, hints: &crate::storage::filter::AxisHints) -> Vec<V>
    where
        V: Send,
    {
        use tagma_core::Coord;

        // Build contiguous prefix from hints
        let mut prefix = Vec::with_capacity(6);
        if let Some(v) = hints.time_hi {
            if let Some(c) = Coord::new(v % 11172) {
                prefix.push(c);
            } else {
                return Vec::new();
            }
        } else {
            return self.values().await;
        } // no prefix possible, full scan

        if let Some(v) = hints.time_lo {
            if let Some(c) = Coord::new(v % 11172) {
                prefix.push(c);
            } else {
                return Vec::new();
            }
        } else {
            return self.values().await;
        }

        if let Some(v) = hints.entity {
            if let Some(c) = Coord::new(v % 11172) {
                prefix.push(c);
            } else {
                return Vec::new();
            }
        } else {
            return self.values().await;
        }

        if let Some(v) = hints.origin {
            if let Some(c) = Coord::new(v % 11172) {
                prefix.push(c);
            } else {
                return Vec::new();
            }
        } else {
            // origin not specified: use prefix up to entity only
            return self
                .iter_prefix_filtered(&prefix, |_| true)
                .await
                .unwrap_or_default();
        }

        if let Some(v) = hints.creator {
            if let Some(c) = Coord::new(v % 11172) {
                prefix.push(c);
            } else {
                return Vec::new();
            }
        } else {
            return self
                .iter_prefix_filtered(&prefix, |_| true)
                .await
                .unwrap_or_default();
        }

        if let Some(v) = hints.serial {
            if let Some(c) = Coord::new(v % 11172) {
                prefix.push(c);
            } else {
                return Vec::new();
            }
        }

        // Full prefix: all 6 axes specified
        self.iter_prefix_filtered(&prefix, |_| true)
            .await
            .unwrap_or_default()
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
