// ── Ordered index and set intersection utils ───────────────────────────
//
// These are the remaining index utilities after FihCoord removal (Phase 3)
// and after the Cell2 interior-mutability primitive moved to chton (the
// behavior layer). OrderedIndex is an append-only key→u32 index kept for
// backward compat; intersect_2/intersect_3 are standalone set-intersection
// helpers.

use alloc::vec::Vec;

// `std::collections::HashSet` exists only under the std feature; alloc has
// no HashSet. The no_std path substitutes a BTreeSet, which satisfies the
// same contract (iteration order is unspecified for HashSet, so callers
// cannot rely on it).
#[cfg(not(feature = "std"))]
use alloc::collections::BTreeSet as HashSet;
#[cfg(feature = "std")]
use std::collections::HashSet;

// Cell2 (platform-adaptive interior mutability) is owned by chton::cell:
// Mutex on native/WASIX, RefCell on wasm32-unknown-unknown. It is
// re-exported here so nexus consumers keep the crate::core::index path.
pub use chton::cell::{Cell2, Ref, RefMut};

// ── OrderedIndex ───────────────────────────────────────────────────

/// Append-only ordered index. Stores compact u32 IDs (no String duplication).
pub struct OrderedIndex<K = u64>
where
    K: Ord + Clone + 'static,
{
    entries: Vec<(K, u32)>,
}

impl<K> OrderedIndex<K>
where
    K: Ord + Clone + 'static,
{
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
    pub fn record(&mut self, key: K, id: u32) {
        self.entries.push((key, id));
    }
    /// All entries with key <= bound.
    pub fn as_of(&self, bound: &K) -> Vec<(K, u32)> {
        self.entries
            .iter()
            .filter(|(k, _)| k <= bound)
            .cloned()
            .collect()
    }
    /// All entries with key > since.
    pub fn since(&self, since: &K) -> Vec<(K, u32)> {
        self.entries
            .iter()
            .filter(|(k, _)| k > since)
            .cloned()
            .collect()
    }
    /// Entries in [lo, hi].
    pub fn range(&self, lo: &K, hi: &K) -> Vec<(K, u32)> {
        self.entries
            .iter()
            .filter(|(k, _)| k >= lo && k <= hi)
            .cloned()
            .collect()
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn first_key(&self) -> Option<K> {
        self.entries.first().map(|(k, _)| k.clone())
    }
    pub fn last_key(&self) -> Option<K> {
        self.entries.last().map(|(k, _)| k.clone())
    }
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl<K> Default for OrderedIndex<K>
where
    K: Ord + Clone + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

// ── Intersection helpers (used by test code) ───────────────────────

pub fn intersect_2(a: &[u32], b: &[u32]) -> Vec<u32> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let (small, large) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let set: HashSet<u32> = HashSet::from_iter(large.iter().copied());
    small
        .iter()
        .filter(|id| set.contains(id))
        .copied()
        .collect()
}

pub fn intersect_3(a: &[u32], b: &[u32], c: &[u32]) -> Vec<u32> {
    if a.is_empty() || b.is_empty() || c.is_empty() {
        return Vec::new();
    }
    // Pick the smallest Vec to iterate, build sets from the other two.
    let (candidates, set1, set2) = if a.len() <= b.len() && a.len() <= c.len() {
        (a, b, c)
    } else if b.len() <= a.len() && b.len() <= c.len() {
        (b, a, c)
    } else {
        (c, a, b)
    };
    let s1: HashSet<u32> = HashSet::from_iter(set1.iter().copied());
    let s2: HashSet<u32> = HashSet::from_iter(set2.iter().copied());
    candidates
        .iter()
        .filter(|id| s1.contains(id) && s2.contains(id))
        .copied()
        .collect()
}
