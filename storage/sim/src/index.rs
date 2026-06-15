// ── OrderedIndex: append-only index over an ordered key space ──────────
//
// A generic temporal/spatial/ordinal index where entries are pushed in
// monotonic order. Binary search via partition_point() provides O(log N)
// seek for range queries (since, as_of, delta flush cursor).
//
// K defaults to u64 (nanosecond timestamps), but can be any Ord type:
//   - u64: default (nanosecond clock, flush cursor)
//   - i64: signed timestamps, version numbers
//   - String: lexicographic keys, logical sequence IDs
//
// Memory: contiguous Vec — cache-friendly, no per-node allocation.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;

/// Append-only ordered index. Thread-local via RefCell.
///
/// Entries must be pushed in monotonically non-decreasing order of K.
/// Binary search assumes this invariant.
pub struct OrderedIndex<K = u64>
where
    K: Ord + Clone + 'static,
{
    entries: RefCell<Vec<(K, String)>>,
}

impl<K> OrderedIndex<K>
where
    K: Ord + Clone + 'static,
{
    pub fn new() -> Self {
        Self {
            entries: RefCell::new(Vec::new()),
        }
    }

    /// Record a new (key, id) pair. O(1) amortized.
    ///
    /// Keys SHOULD be pushed in monotonically non-decreasing order, but this
    /// is not enforced — out-of-order entries will still be stored and the
    /// binary-search range methods will continue to work correctly, though
    /// the seek position may shift depending on the degree of disorder.
    pub fn record(&self, key: K, id: &str) {
        self.entries.borrow_mut().push((key, id.to_string()));
    }

    /// Return all entries with key <= bound (time-travel "as of").
    /// O(log N) seek + O(K) output.
    pub fn as_of(&self, bound: &K) -> Vec<(K, String)> {
        let entries = self.entries.borrow();
        let end = entries.partition_point(|(k, _)| *k <= *bound);
        entries[..end].to_vec()
    }

    /// Return all entries with key > bound (delta since cursor).
    /// O(log N) seek + O(K) output.
    pub fn since(&self, bound: &K) -> Vec<(K, String)> {
        let entries = self.entries.borrow();
        let start = entries.partition_point(|(k, _)| *k <= *bound);
        entries[start..].to_vec()
    }

    /// Return all entries with start <= key < end.
    /// O(log N) seek + O(K) output.
    pub fn range(&self, start: &K, end: &K) -> Vec<(K, String)> {
        let entries = self.entries.borrow();
        let start_idx = entries.partition_point(|(k, _)| *k < *start);
        let end_idx = entries.partition_point(|(k, _)| *k < *end);
        entries[start_idx..end_idx].to_vec()
    }

    /// Total number of recorded entries. O(1).
    pub fn len(&self) -> usize {
        self.entries.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.borrow().is_empty()
    }

    /// First key in the index, or None if empty. O(1).
    pub fn first_key(&self) -> Option<K> {
        self.entries
            .borrow()
            .first()
            .map(|(k, _)| k.clone())
    }

    /// Last key in the index, or None if empty. O(1).
    pub fn last_key(&self) -> Option<K> {
        self.entries
            .borrow()
            .last()
            .map(|(k, _)| k.clone())
    }

    /// Drain all entries (for testing). O(1).
    pub fn clear(&self) {
        self.entries.borrow_mut().clear();
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

// ── FihCoord: composite coordinate/index for FIH StateSpace ────────────
//
// Groups all derived projections (time, origin, fact→intent, ref_count)
// into a single structure. Always in-memory, rebuilt from EntityStore on
// hydrate. NOT persisted — indices are reconstructed from Record data.
//
// Each field represents one axis or relationship in the Sparse StateSpace:
//   by_time:     temporal axis (4th dimension)
//   by_origin:   origin → [fact_id] projection (fact space)
//   by_fact:     fact_id → [intent_id] reverse index (fact→intent relation)
//   ref_counts:  fact_id → number of referencing Intents

/// Reference counts: fact_id → number of Intents referencing this Fact.
pub(crate) type RefCounts = HashMap<String, AtomicU64>;

/// Origin index: origin → [fact_id, ...]
pub(crate) type OriginIndex = HashMap<String, Vec<String>>;

/// Reverse index: fact_id → intent_id list.
pub(crate) type ByFactIndex = HashMap<String, Vec<String>>;

/// FIH StateSpace coordinate: composite index over all non-record axes.
///
/// Replaces the previous four separate fields (time_index, ref_counts,
/// by_origin, by_from_fact) in FihStorage. Each field is an in-memory
/// projection of the Record data, rebuilt via rebuild_coord().
pub struct FihCoord {
    /// Temporal axis: timestamp → id (monotonic, append-only)
    pub by_time: OrderedIndex<u64>,

    /// Origin projection: origin → [fact_id]
    pub by_origin: RefCell<OriginIndex>,

    /// Fact reverse index: fact_id → [intent_id]
    pub by_fact: RefCell<ByFactIndex>,

    /// Reference count: fact_id → #referencing Intents
    pub ref_counts: RefCell<RefCounts>,
}

impl FihCoord {
    pub fn new() -> Self {
        Self {
            by_time: OrderedIndex::<u64>::new(),
            by_origin: RefCell::new(OriginIndex::new()),
            by_fact: RefCell::new(ByFactIndex::new()),
            ref_counts: RefCell::new(RefCounts::new()),
        }
    }

    /// Return intent IDs referencing the given fact (reverse index lookup).
    pub fn intents_by_fact(&self, fact_id: &str) -> Vec<String> {
        self.by_fact
            .borrow()
            .get(fact_id)
            .cloned()
            .unwrap_or_default()
    }
}

impl Default for FihCoord {
    fn default() -> Self {
        Self::new()
    }
}
