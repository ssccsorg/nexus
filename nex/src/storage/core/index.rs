use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};

use crate::storage::semantic::{FihLoad, FihQuery, SemanticStore};
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
        Self {
            entries: RefCell::new(Vec::new()),
        }
    }
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
    pub fn len(&self) -> usize {
        self.entries.borrow().len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.borrow().is_empty()
    }
    pub fn first_key(&self) -> Option<K> {
        self.entries.borrow().first().map(|(k, _)| k.clone())
    }
    pub fn last_key(&self) -> Option<K> {
        self.entries.borrow().last().map(|(k, _)| k.clone())
    }
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

/// String interner: deduplicates strings into u32 IDs.
struct StringInterner {
    map: HashMap<String, u32>,
    vec: Vec<String>,
}
impl StringInterner {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            vec: Vec::new(),
        }
    }
    fn intern(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.map.get(s) {
            return id;
        }
        let id = self.vec.len() as u32;
        self.map.insert(s.to_string(), id);
        self.vec.push(s.to_string());
        id
    }
    fn get(&self, s: &str) -> Option<u32> {
        self.map.get(s).copied()
    }
    fn clear(&mut self) {
        self.map.clear();
        self.vec.clear();
    }
}

// ── FihCoord ────────────────────────────────────────────────────────────

pub struct FihCoord {
    pub(crate) id_to_idx: RefCell<HashMap<[u8; 32], u32>>,
    pub(crate) idx_to_id: RefCell<Vec<String>>,
    next_idx: Cell<u32>,

    /// String interner for origin, creator, status keys.
    str_interner: RefCell<StringInterner>,

    pub by_time: OrderedIndex<u64>,
    /// Interned origin string → compact IDs.
    pub by_origin: RefCell<HashMap<u32, Vec<u32>>>,
    pub by_fact: RefCell<HashMap<u32, Vec<u32>>>,
    /// Interned creator string → compact IDs.
    pub by_creator: RefCell<HashMap<u32, Vec<u32>>>,
    /// Interned status string → intent compact IDs.
    pub by_status: RefCell<HashMap<u32, Vec<u32>>>,
    pub by_created_at_day: RefCell<BTreeMap<u64, Vec<u32>>>,
    pub ref_counts: RefCell<HashMap<u32, Cell<u64>>>,
    /// Semantic feature store for similarity search (plug-in).
    pub by_semantic: RefCell<Vec<Box<dyn SemanticStore>>>,
}

impl FihCoord {
    pub fn new() -> Self {
        Self {
            id_to_idx: RefCell::new(HashMap::new()),
            idx_to_id: RefCell::new(Vec::new()),
            next_idx: Cell::new(0),
            str_interner: RefCell::new(StringInterner::new()),
            by_time: OrderedIndex::<u64>::new(),
            by_origin: RefCell::new(HashMap::new()),
            by_fact: RefCell::new(HashMap::new()),
            by_creator: RefCell::new(HashMap::new()),
            by_status: RefCell::new(HashMap::new()),
            by_created_at_day: RefCell::new(BTreeMap::new()),
            ref_counts: RefCell::new(HashMap::new()),
            by_semantic: RefCell::new(Vec::new()),
        }
    }

    pub fn intern(&self, hash: &[u8; 32]) -> u32 {
        // Fast path: already interned (read-only borrow)
        if let Some(&idx) = self.id_to_idx.borrow().get(hash) {
            return idx;
        }
        // Slow path: new hash (mutable borrow)
        let idx = self.next_idx.get();
        self.next_idx.set(idx + 1);
        self.id_to_idx.borrow_mut().insert(*hash, idx);
        let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
        self.idx_to_id.borrow_mut().push(hex);
        idx
    }

    pub fn intern_str(&self, id: &str) -> u32 {
        let hash = FihHash::from_hex(id);
        self.intern(&hash.0)
    }

    /// Intern an origin/creator/status string → u32.
    pub fn intern_str_key(&self, s: &str) -> u32 {
        // Fast path: already interned (read-only borrow)
        if let Some(id) = self.str_interner.borrow().get(s) {
            return id;
        }
        // Slow path: new string (mutable borrow)
        self.str_interner.borrow_mut().intern(s)
    }

    /// Look up an interned string key without inserting. Returns None if not found.
    pub fn lookup_str_key(&self, s: &str) -> Option<u32> {
        self.str_interner.borrow().get(s)
    }

    pub fn resolve(&self, idx: u32) -> String {
        self.idx_to_id
            .borrow()
            .get(idx as usize)
            .cloned()
            .unwrap_or_default()
    }

    pub fn clear(&self) {
        self.id_to_idx.borrow_mut().clear();
        self.idx_to_id.borrow_mut().clear();
        self.next_idx.set(0);
        self.str_interner.borrow_mut().clear();
        self.by_time.clear();
        self.by_origin.borrow_mut().clear();
        self.by_fact.borrow_mut().clear();
        self.by_creator.borrow_mut().clear();
        self.by_status.borrow_mut().clear();
        self.by_created_at_day.borrow_mut().clear();
        self.ref_counts.borrow_mut().clear();
        self.by_semantic.borrow_mut().clear();
    }

    // ── Index update ───────────────────────────────────────────────

    pub fn record_fact(&self, id: &[u8; 32], origin: &str, creator: &str, created_at: u64) {
        let idx = self.intern(id);
        self.by_time.record(created_at, idx);
        let oid = self.intern_str_key(origin);
        self.by_origin
            .borrow_mut()
            .entry(oid)
            .or_default()
            .push(idx);
        let cid = self.intern_str_key(creator);
        self.by_creator
            .borrow_mut()
            .entry(cid)
            .or_default()
            .push(idx);
        let day = created_at - (created_at % 86_400_000_000_000);
        self.by_created_at_day
            .borrow_mut()
            .entry(day)
            .or_default()
            .push(idx);
        self.ref_counts
            .borrow_mut()
            .entry(idx)
            .or_insert_with(|| Cell::new(0));
    }

    pub fn record_intent(
        &self,
        id: &[u8; 32],
        creator: &str,
        created_at: u64,
        from_facts: &[[u8; 32]],
    ) {
        let idx = self.intern(id);
        let cid = self.intern_str_key(creator);
        self.by_creator
            .borrow_mut()
            .entry(cid)
            .or_default()
            .push(idx);
        let sid = self.intern_str_key("submitted");
        self.by_status
            .borrow_mut()
            .entry(sid)
            .or_default()
            .push(idx);
        let day = created_at - (created_at % 86_400_000_000_000);
        self.by_created_at_day
            .borrow_mut()
            .entry(day)
            .or_default()
            .push(idx);
        for fid in from_facts {
            let fact_idx = self.intern(fid);
            self.by_fact
                .borrow_mut()
                .entry(fact_idx)
                .or_default()
                .push(idx);
            if let Some(rc) = self.ref_counts.borrow().get(&fact_idx) {
                rc.set(rc.get() + 1);
            }
        }
    }

    pub fn update_intent_status(&self, id: &[u8; 32], old_status: &str, new_status: &str) {
        let idx = self.intern(id);
        let old_sid = self.intern_str_key(old_status);
        let new_sid = self.intern_str_key(new_status);
        {
            let mut by_status = self.by_status.borrow_mut();
            if let Some(bucket) = by_status.get_mut(&old_sid) {
                bucket.retain(|&i| i != idx);
            }
        }
        self.by_status
            .borrow_mut()
            .entry(new_sid)
            .or_default()
            .push(idx);
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

    // ── Semantic store interaction ────────────────────────────────

    /// Insert a record into the semantic store using the provided `FihLoad`.
    pub fn semantic_insert(&self, id: u32, load: &dyn FihLoad) -> Result<(), String> {
        let mut stores = self.by_semantic.borrow_mut();
        if stores.is_empty() {
            return Err("no semantic stores configured".into());
        }
        let num_stores = stores.len();
        for store in stores.iter_mut() {
            store
                .insert(id, load)
                .map_err(|e| format!("semantic insert failed (store {num_stores}): {e}"))?;
        }
        Ok(())
    }

    /// Search the semantic store using the provided query handle.
    pub fn semantic_search(
        &self,
        query: &dyn FihQuery,
        top_k: usize,
    ) -> Result<Vec<(u32, f32)>, String> {
        let stores = self.by_semantic.borrow();
        if stores.is_empty() {
            return Err("no semantic stores configured".into());
        }
        let mut all_results = Vec::new();
        for store in stores.iter() {
            if let Ok(results) = store.search(query, top_k) {
                all_results.extend(results);
            }
        }
        // Sort by score descending and take top_k
        all_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        all_results.truncate(top_k);
        Ok(all_results)
    }

    // ── Query ──────────────────────────────────────────────────────

    pub fn facts_by_creator(&self, creator: &str) -> Vec<u32> {
        match self.lookup_str_key(creator) {
            Some(cid) => self
                .by_creator
                .borrow()
                .get(&cid)
                .cloned()
                .unwrap_or_default(),
            None => Vec::new(),
        }
    }
    pub fn intents_by_status(&self, status: &str) -> Vec<u32> {
        match self.lookup_str_key(status) {
            Some(sid) => self
                .by_status
                .borrow()
                .get(&sid)
                .cloned()
                .unwrap_or_default(),
            None => Vec::new(),
        }
    }
    pub fn ids_by_created_at_range(&self, start_day: u64, end_day: u64) -> Vec<u32> {
        let mut result = Vec::new();
        for (_day, ids) in self.by_created_at_day.borrow().range(start_day..end_day) {
            result.extend(ids.iter().copied());
        }
        result
    }
    pub fn intents_by_fact(&self, fact_idx: u32) -> Vec<u32> {
        self.by_fact
            .borrow()
            .get(&fact_idx)
            .cloned()
            .unwrap_or_default()
    }
    pub fn fact_ids_by_origin(&self, origin: &str) -> Vec<u32> {
        let oid = self.intern_str_key(origin);
        self.by_origin
            .borrow()
            .get(&oid)
            .cloned()
            .unwrap_or_default()
    }
}

impl Default for FihCoord {
    fn default() -> Self {
        Self::new()
    }
}
