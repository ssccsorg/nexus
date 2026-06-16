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

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};

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
        self.entries.borrow().first().map(|(k, _)| k.clone())
    }

    /// Last key in the index, or None if empty. O(1).
    pub fn last_key(&self) -> Option<K> {
        self.entries.borrow().last().map(|(k, _)| k.clone())
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
// Uses a unified u32 ID mapping internally so that all indexes
// (exact-match, range, and future vector indexes) share the same
// compact ID space. The mapping is rebuilt from record data on hydrate.
//
// Always in-memory, rebuilt from EntityStore on hydrate. NOT persisted
// — indices are reconstructed from Record data.
//
// Each field represents one axis or relationship in the Sparse StateSpace:
//   by_time:            temporal axis (4th dimension)
//   by_origin:          origin → [compact_id] projection (fact space)
//   by_fact:            fact_id → [intent compact_id] reverse index
//   by_creator:         creator → [compact_id]
//   by_status:          status_string → [intent compact_id]
//   by_created_at_day:  day-precision key → [compact_id] (range queries)
//   ref_counts:         compact_id → reference count

/// FIH StateSpace coordinate: composite index over all record axes.
///
/// Uses a unified u32 ID mapping internally so that all indexes
/// (exact-match, range, and future vector indexes) share the same
/// compact ID space. The mapping is rebuilt from record data on hydrate.
pub struct FihCoord {
    // ── Unified ID mapping (shared by all indexes) ─────────────────
    /// String ID → compact u32 index
    pub(crate) id_to_idx: RefCell<HashMap<String, u32>>,
    /// u32 index → String ID (reverse lookup)
    pub(crate) idx_to_id: RefCell<Vec<String>>,
    /// Next available u32 ID (monotonic)
    next_idx: Cell<u32>,

    // ── Indexes ────────────────────────────────────────────────────
    /// Temporal axis: timestamp → id (monotonic, append-only)
    pub by_time: OrderedIndex<u64>,

    /// Origin projection: origin → [compact_id]
    pub by_origin: RefCell<HashMap<String, Vec<u32>>>,

    /// Fact reverse index: fact_id → [intent compact_id]
    pub by_fact: RefCell<HashMap<u32, Vec<u32>>>,

    /// Creator projection: creator → [compact_id]
    pub by_creator: RefCell<HashMap<String, Vec<u32>>>,

    /// Intent status projection: status_string → [intent compact_id]
    pub by_status: RefCell<HashMap<String, Vec<u32>>>,

    /// Day-precision timestamp index: day → [compact_id] (range queries)
    pub by_created_at_day: RefCell<BTreeMap<u64, Vec<u32>>>,

    /// Reference count: compact_id → count
    pub ref_counts: RefCell<HashMap<u32, Cell<u64>>>,
}

impl FihCoord {
    pub fn new() -> Self {
        Self {
            id_to_idx: RefCell::new(HashMap::new()),
            idx_to_id: RefCell::new(Vec::new()),
            next_idx: Cell::new(0),
            by_time: OrderedIndex::<u64>::new(),
            by_origin: RefCell::new(HashMap::new()),
            by_fact: RefCell::new(HashMap::new()),
            by_creator: RefCell::new(HashMap::new()),
            by_status: RefCell::new(HashMap::new()),
            by_created_at_day: RefCell::new(BTreeMap::new()),
            ref_counts: RefCell::new(HashMap::new()),
        }
    }

    /// Get or create a compact ID for a string ID.
    pub fn intern(&self, id: &str) -> u32 {
        let mut map = self.id_to_idx.borrow_mut();
        if let Some(&idx) = map.get(id) {
            return idx;
        }
        let idx = self.next_idx.get();
        self.next_idx.set(idx + 1);
        map.insert(id.to_string(), idx);
        self.idx_to_id.borrow_mut().push(id.to_string());
        idx
    }

    /// Resolve a compact ID back to its string ID.
    pub fn resolve(&self, idx: u32) -> String {
        self.idx_to_id
            .borrow()
            .get(idx as usize)
            .cloned()
            .unwrap_or_default()
    }

    /// Clear all indexes and ID mapping (for rebuild).
    pub fn clear(&self) {
        self.id_to_idx.borrow_mut().clear();
        self.idx_to_id.borrow_mut().clear();
        self.next_idx.set(0);
        self.by_time.clear();
        self.by_origin.borrow_mut().clear();
        self.by_fact.borrow_mut().clear();
        self.by_creator.borrow_mut().clear();
        self.by_status.borrow_mut().clear();
        self.by_created_at_day.borrow_mut().clear();
        self.ref_counts.borrow_mut().clear();
    }

    // ── Index update methods (called by FihStorage) ────────────────

    /// Record a fact in all applicable indexes.
    pub fn record_fact(&self, id: &str, origin: &str, creator: &str, created_at: u64) {
        let idx = self.intern(id);

        // Origin projection
        self.by_origin
            .borrow_mut()
            .entry(origin.to_string())
            .or_default()
            .push(idx);

        // Creator projection
        self.by_creator
            .borrow_mut()
            .entry(creator.to_string())
            .or_default()
            .push(idx);

        // Day-precision timestamp index (truncate to day boundary)
        let day = created_at - (created_at % 86_400_000_000_000);
        self.by_created_at_day
            .borrow_mut()
            .entry(day)
            .or_default()
            .push(idx);

        // Initialize reference count (0 — orphan unless referenced by an Intent)
        self.ref_counts
            .borrow_mut()
            .entry(idx)
            .or_insert_with(|| Cell::new(0));
    }

    /// Record an intent in all applicable indexes.
    pub fn record_intent(
        &self,
        id: &str,
        creator: &str,
        created_at: u64,
        from_facts: &[String],
    ) {
        let idx = self.intern(id);

        // Creator projection
        self.by_creator
            .borrow_mut()
            .entry(creator.to_string())
            .or_default()
            .push(idx);

        // Status projection (intents start as "submitted")
        self.by_status
            .borrow_mut()
            .entry("submitted".to_string())
            .or_default()
            .push(idx);

        // Day-precision timestamp index (truncate to day boundary)
        let day = created_at - (created_at % 86_400_000_000_000);
        self.by_created_at_day
            .borrow_mut()
            .entry(day)
            .or_default()
            .push(idx);

        // Fact reverse index: for each referenced fact, record this intent
        for fid in from_facts {
            let fact_idx = self.intern(fid);
            self.by_fact
                .borrow_mut()
                .entry(fact_idx)
                .or_default()
                .push(idx);

            // Increment reference count on the fact
            if let Some(rc) = self.ref_counts.borrow().get(&fact_idx) {
                rc.set(rc.get() + 1);
            }
        }
    }

    /// Update intent status index (called on claim/conclude).
    pub fn update_intent_status(&self, id: &str, old_status: &str, new_status: &str) {
        let idx = self.intern(id);

        // Remove from old status bucket
        {
            let mut by_status = self.by_status.borrow_mut();
            if let Some(bucket) = by_status.get_mut(old_status) {
                bucket.retain(|&i| i != idx);
            }
        }

        // Add to new status bucket
        self.by_status
            .borrow_mut()
            .entry(new_status.to_string())
            .or_default()
            .push(idx);
    }

    /// Remove intent from old from_facts references (called on conclude).
    pub fn remove_intent_from_facts(&self, id: &str, from_facts: &[String]) {
        let idx = self.intern(id);
        let mut by_fact = self.by_fact.borrow_mut();
        for fid in from_facts {
            let fact_idx = self.intern(fid);
            if let Some(bucket) = by_fact.get_mut(&fact_idx) {
                bucket.retain(|&i| i != idx);
            }

            // Decrement reference count on the fact
            if let Some(rc) = self.ref_counts.borrow().get(&fact_idx) {
                rc.set(rc.get() - 1);
            }
        }
    }

    // ── Query methods ──────────────────────────────────────────────

    /// Return compact IDs for facts created by a given creator.
    pub fn facts_by_creator(&self, creator: &str) -> Vec<u32> {
        self.by_creator
            .borrow()
            .get(creator)
            .cloned()
            .unwrap_or_default()
    }

    /// Return compact IDs for intents with a given status.
    pub fn intents_by_status(&self, status: &str) -> Vec<u32> {
        self.by_status
            .borrow()
            .get(status)
            .cloned()
            .unwrap_or_default()
    }

    /// Return compact IDs for records created within a day range (inclusive of start, exclusive of end).
    pub fn ids_by_created_at_range(&self, start_day: u64, end_day: u64) -> Vec<u32> {
        let map = self.by_created_at_day.borrow();
        let mut result = Vec::new();
        for (_day, ids) in map.range(start_day..end_day) {
            result.extend(ids.iter().copied());
        }
        result
    }

    /// Return compact IDs for intents referencing a given fact (by its compact ID).
    pub fn intents_by_fact(&self, fact_idx: u32) -> Vec<u32> {
        self.by_fact
            .borrow()
            .get(&fact_idx)
            .cloned()
            .unwrap_or_default()
    }

    /// Return compact IDs for facts with a given origin.
    pub fn fact_ids_by_origin(&self, origin: &str) -> Vec<u32> {
        self.by_origin
            .borrow()
            .get(origin)
            .cloned()
            .unwrap_or_default()
    }
}

impl Default for FihCoord {
    fn default() -> Self {
        Self::new()
    }
}
