use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};

use nexus_model::FihHash;

/// Append-only ordered index. Stores compact u32 IDs (no String duplication).
pub struct OrderedIndex<K = u64>
where
    K: Ord + Clone + 'static,
{
    entries: RefCell<Vec<(K, u32)>>,
}

impl<K> OrderedIndex<K>
where
    K: Ord + Clone + 'static,
{
    pub fn new() -> Self {
        Self { entries: RefCell::new(Vec::new()) }
    }
    /// Record a (key, compact_id) pair. O(1) amortized.
    pub fn record(&self, key: K, id: u32) {
        self.entries.borrow_mut().push((key, id));
    }
    pub fn as_of(&self, bound: &K) -> Vec<(K, u32)> {
        let entries = self.entries.borrow();
        let end = entries.partition_point(|(k, _)| *k <= *bound);
        entries[..end].to_vec()
    }
    pub fn since(&self, bound: &K) -> Vec<(K, u32)> {
        let entries = self.entries.borrow();
        let start = entries.partition_point(|(k, _)| *k <= *bound);
        entries[start..].to_vec()
    }
    pub fn range(&self, start: &K, end: &K) -> Vec<(K, u32)> {
        let entries = self.entries.borrow();
        let start_idx = entries.partition_point(|(k, _)| *k < *start);
        let end_idx = entries.partition_point(|(k, _)| *k < *end);
        entries[start_idx..end_idx].to_vec()
    }
    pub fn len(&self) -> usize { self.entries.borrow().len() }
    pub fn is_empty(&self) -> bool { self.entries.borrow().is_empty() }
    pub fn first_key(&self) -> Option<K> {
        self.entries.borrow().first().map(|(k, _)| k.clone())
    }
    pub fn last_key(&self) -> Option<K> {
        self.entries.borrow().last().map(|(k, _)| k.clone())
    }
    pub fn clear(&self) { self.entries.borrow_mut().clear(); }
}

impl<K> Default for OrderedIndex<K>
where K: Ord + Clone + 'static,
{ fn default() -> Self { Self::new() } }

// ── FihCoord ────────────────────────────────────────────────────────────

pub struct FihCoord {
    pub(crate) id_to_idx: RefCell<HashMap<[u8; 32], u32>>,
    pub(crate) idx_to_id: RefCell<Vec<String>>,
    next_idx: Cell<u32>,

    pub by_time: OrderedIndex<u64>,
    pub by_origin: RefCell<HashMap<String, Vec<u32>>>,
    pub by_fact: RefCell<HashMap<u32, Vec<u32>>>,
    pub by_creator: RefCell<HashMap<String, Vec<u32>>>,
    pub by_status: RefCell<HashMap<String, Vec<u32>>>,
    pub by_created_at_day: RefCell<BTreeMap<u64, Vec<u32>>>,
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

    pub fn intern(&self, hash: &[u8; 32]) -> u32 {
        let mut map = self.id_to_idx.borrow_mut();
        if let Some(&idx) = map.get(hash) { return idx; }
        let idx = self.next_idx.get();
        self.next_idx.set(idx + 1);
        map.insert(*hash, idx);
        let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
        self.idx_to_id.borrow_mut().push(hex);
        idx
    }

    pub fn intern_str(&self, id: &str) -> u32 {
        let hash = FihHash::from_hex(id);
        self.intern(&hash.0)
    }

    pub fn resolve(&self, idx: u32) -> String {
        self.idx_to_id.borrow().get(idx as usize).cloned().unwrap_or_default()
    }

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

    // ── Index update ───────────────────────────────────────────────

    pub fn record_fact(&self, id: &[u8; 32], origin: &str, creator: &str, created_at: u64) {
        let idx = self.intern(id);
        self.by_time.record(created_at, idx);
        self.by_origin.borrow_mut().entry(origin.to_string()).or_default().push(idx);
        self.by_creator.borrow_mut().entry(creator.to_string()).or_default().push(idx);
        let day = created_at - (created_at % 86_400_000_000_000);
        self.by_created_at_day.borrow_mut().entry(day).or_default().push(idx);
        self.ref_counts.borrow_mut().entry(idx).or_insert_with(|| Cell::new(0));
    }

    pub fn record_intent(&self, id: &[u8; 32], creator: &str, created_at: u64, from_facts: &[[u8; 32]]) {
        let idx = self.intern(id);
        self.by_creator.borrow_mut().entry(creator.to_string()).or_default().push(idx);
        self.by_status.borrow_mut().entry("submitted".to_string()).or_default().push(idx);
        let day = created_at - (created_at % 86_400_000_000_000);
        self.by_created_at_day.borrow_mut().entry(day).or_default().push(idx);
        for fid in from_facts {
            let fact_idx = self.intern(fid);
            self.by_fact.borrow_mut().entry(fact_idx).or_default().push(idx);
            if let Some(rc) = self.ref_counts.borrow().get(&fact_idx) {
                rc.set(rc.get() + 1);
            }
        }
    }

    pub fn update_intent_status(&self, id: &[u8; 32], old_status: &str, new_status: &str) {
        let idx = self.intern(id);
        {
            let mut by_status = self.by_status.borrow_mut();
            if let Some(bucket) = by_status.get_mut(old_status) {
                bucket.retain(|&i| i != idx);
            }
        }
        self.by_status.borrow_mut().entry(new_status.to_string()).or_default().push(idx);
    }

    pub fn remove_intent_from_facts(&self, id: &[u8; 32], from_facts: &[[u8; 32]]) {
        let idx = self.intern(id);
        let mut by_fact = self.by_fact.borrow_mut();
        for fid in from_facts {
            let fact_idx = self.intern(fid);
            if let Some(bucket) = by_fact.get_mut(&fact_idx) {
                bucket.retain(|&i| i != idx);
            }
            if let Some(rc) = self.ref_counts.borrow().get(&fact_idx) {
                rc.set(rc.get() - 1);
            }
        }
    }

    // ── Query ──────────────────────────────────────────────────────

    pub fn facts_by_creator(&self, creator: &str) -> Vec<u32> {
        self.by_creator.borrow().get(creator).cloned().unwrap_or_default()
    }
    pub fn intents_by_status(&self, status: &str) -> Vec<u32> {
        self.by_status.borrow().get(status).cloned().unwrap_or_default()
    }
    pub fn ids_by_created_at_range(&self, start_day: u64, end_day: u64) -> Vec<u32> {
        let mut result = Vec::new();
        for (_day, ids) in self.by_created_at_day.borrow().range(start_day..end_day) {
            result.extend(ids.iter().copied());
        }
        result
    }
    pub fn intents_by_fact(&self, fact_idx: u32) -> Vec<u32> {
        self.by_fact.borrow().get(&fact_idx).cloned().unwrap_or_default()
    }
    pub fn fact_ids_by_origin(&self, origin: &str) -> Vec<u32> {
        self.by_origin.borrow().get(origin).cloned().unwrap_or_default()
    }
}

impl Default for FihCoord {
    fn default() -> Self { Self::new() }
}
