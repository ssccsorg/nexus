use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::AtomicU32;

use crate::storage::semantic::{DynSemanticStore, Query, RecordLoad};
use nexus_model::FihHash;

// ── Internal cell type ─────────────────────────────────────────────
//
// On native/WASIX (where std is available): std::sync::Mutex
// On wasm32-unknown-unknown:                   std::cell::RefCell
//
// FihStorage and FihCoord are Send+Sync on native, single-threaded on wasm.
// The public API is identical regardless of platform.

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub type RefMut<'a, T> = std::sync::MutexGuard<'a, T>;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub type RefMut<'a, T> = std::cell::RefMut<'a, T>;

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub type Ref<'a, T> = std::sync::MutexGuard<'a, T>;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub type Ref<'a, T> = std::cell::Ref<'a, T>;

/// Platform-adaptive cell: Mutex on native/WASIX, RefCell on wasm32-unknown-unknown.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub struct Cell2<T>(std::sync::Mutex<T>);

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub struct Cell2<T>(std::cell::RefCell<T>);

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl<T> Cell2<T> {
    pub fn new(val: T) -> Self {
        Self(std::sync::Mutex::new(val))
    }
    pub fn borrow(&self) -> Ref<'_, T> {
        self.0.lock().expect("Cell2 lock")
    }
    pub fn borrow_mut(&self) -> RefMut<'_, T> {
        self.0.lock().expect("Cell2 lock")
    }
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
impl<T> Cell2<T> {
    pub fn new(val: T) -> Self {
        Self(std::cell::RefCell::new(val))
    }
    pub fn borrow(&self) -> Ref<'_, T> {
        self.0.borrow()
    }
    pub fn borrow_mut(&self) -> RefMut<'_, T> {
        self.0.borrow_mut()
    }
}

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
    pub fn as_of(&self, bound: &K) -> Vec<(K, u32)> {
        let end = self.entries.partition_point(|(k, _)| *k <= *bound);
        self.entries[..end].to_vec()
    }
    pub fn since(&self, bound: &K) -> Vec<(K, u32)> {
        let start = self.entries.partition_point(|(k, _)| *k <= *bound);
        self.entries[start..].to_vec()
    }
    pub fn range(&self, start: &K, end: &K) -> Vec<(K, u32)> {
        let start_idx = self.entries.partition_point(|(k, _)| *k < *start);
        let end_idx = self.entries.partition_point(|(k, _)| *k < *end);
        self.entries[start_idx..end_idx].to_vec()
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

// ── FihCoord ────────────────────────────────────────────────────────────

pub struct FihCoord {
    pub(crate) id_to_idx: Cell2<HashMap<[u8; 32], u32>>,
    pub(crate) idx_to_id: Cell2<Vec<String>>,
    next_idx: AtomicU32,

    /// String interner for origin, creator, status keys.
    str_interner: Cell2<StringInterner>,

    /// Interned origin string → compact IDs.
    pub(crate) by_origin: Cell2<HashMap<u32, Vec<u32>>>,
    pub(crate) by_fact: Cell2<HashMap<u32, Vec<u32>>>,
    /// Interned creator string → compact IDs.
    pub(crate) by_creator: Cell2<HashMap<u32, Vec<u32>>>,
    /// Interned status string → intent compact IDs.
    pub(crate) by_status: Cell2<HashMap<u32, Vec<u32>>>,
    pub(crate) by_created_at_day: Cell2<BTreeMap<u64, Vec<u32>>>,
    pub(crate) ref_counts: Cell2<HashMap<u32, u64>>,
    /// Semantic feature store for similarity search (plug-in).
    pub(crate) by_semantic: Cell2<Vec<DynSemanticStore>>,
    pub(crate) by_time: Cell2<OrderedIndex<u64>>,
}

impl FihCoord {
    pub fn new() -> Self {
        Self {
            id_to_idx: Cell2::new(HashMap::new()),
            idx_to_id: Cell2::new(Vec::new()),
            next_idx: AtomicU32::new(0),
            str_interner: Cell2::new(StringInterner::new()),
            by_time: Cell2::new(OrderedIndex::<u64>::new()),
            by_origin: Cell2::new(HashMap::new()),
            by_fact: Cell2::new(HashMap::new()),
            by_creator: Cell2::new(HashMap::new()),
            by_status: Cell2::new(HashMap::new()),
            by_created_at_day: Cell2::new(BTreeMap::new()),
            ref_counts: Cell2::new(HashMap::new()),
            by_semantic: Cell2::new(Vec::new()),
        }
    }

    pub fn intern(&self, hash: &[u8; 32]) -> u32 {
        // Fast path: already interned (read-only borrow).
        // TOCTOU is benign here because insert overwrites existing keys,
        // and next_idx only moves forward (index waste is acceptable).
        if let Some(&idx) = self.id_to_idx.borrow().get(hash) {
            return idx;
        }
        // Slow path: new hash (single mutable borrow, no TOCTOU window).
        let mut map = self.id_to_idx.borrow_mut();
        if let Some(&idx) = map.get(hash) {
            return idx;
        }
        let idx = self
            .next_idx
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        map.insert(*hash, idx);
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

    /// Clear all indices EXCEPT semantic stores.
    pub fn clear(&self) {
        self.id_to_idx.borrow_mut().clear();
        self.idx_to_id.borrow_mut().clear();
        self.next_idx.store(0, std::sync::atomic::Ordering::Relaxed);
        self.str_interner.borrow_mut().clear();
        self.by_time.borrow_mut().clear();
        self.by_origin.borrow_mut().clear();
        self.by_fact.borrow_mut().clear();
        self.by_creator.borrow_mut().clear();
        self.by_status.borrow_mut().clear();
        self.by_created_at_day.borrow_mut().clear();
        self.ref_counts.borrow_mut().clear();
        // by_semantic is NOT cleared here — semantic stores persist
        // across index rebuilds.
    }

    /// Clear semantic stores.
    pub fn clear_semantic(&self) {
        self.by_semantic.borrow_mut().clear();
    }

    /// Add a semantic store. Used by tests and external setup.
    pub fn add_semantic_store(&self, store: DynSemanticStore) {
        self.by_semantic.borrow_mut().push(store);
    }

    // ── Index update ──────────────────────────────────────────────

    pub fn record_fact(&self, id: &[u8; 32], origin: &str, creator: &str, created_at: u64) {
        let idx = self.intern(id);
        self.by_time.borrow_mut().record(created_at, idx);
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
        self.ref_counts.borrow_mut().entry(idx).or_insert(0);
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
            let mut ref_counts = self.ref_counts.borrow_mut();
            if let Some(rc) = ref_counts.get_mut(&fact_idx) {
                *rc += 1;
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
            let mut ref_counts = self.ref_counts.borrow_mut();
            if let Some(rc) = ref_counts.get_mut(&fact_idx) {
                *rc = rc.saturating_sub(1);
            }
        }
    }

    // ── Semantic store interaction ────────────────────────────────

    pub async fn semantic_insert(&self, id: u32, load: &dyn RecordLoad) -> Result<(), String> {
        // Take stores atomically, work on them, then swap back.
        // Race: another concurrent semantic_insert may overwrite this result.
        // In practice this is rare; add_semantic_store is called at startup only.
        let mut stores = std::mem::take(&mut *self.by_semantic.borrow_mut());
        if stores.is_empty() {
            return Err("no semantic stores configured".into());
        }
        let mut last_err: Option<String> = None;
        for store in stores.iter_mut() {
            if let Err(e) = store.insert(id, load).await {
                last_err = Some(e);
            }
        }
        // Swap back (not extend) to avoid clobbering stores added by other threads.
        self.by_semantic.borrow_mut().extend(stores);
        if let Some(e) = last_err {
            Err(e)
        } else {
            Ok(())
        }
    }

    pub async fn semantic_search(
        &self,
        query: &dyn Query,
        top_k: usize,
    ) -> Result<Vec<(u32, f32)>, String> {
        // Take stores atomically, search, then swap back.
        let stores = std::mem::take(&mut *self.by_semantic.borrow_mut());
        if stores.is_empty() {
            return Err("no semantic stores configured".into());
        }
        let mut all_results = Vec::new();
        for store in stores.iter() {
            if let Ok(results) = store.search(query, top_k).await {
                all_results.extend(results);
            }
        }
        // Swap back (not extend) to avoid clobbering stores added by other threads.
        self.by_semantic.borrow_mut().extend(stores);
        all_results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        all_results.truncate(top_k);
        Ok(all_results)
    }

    // ── Query ─────────────────────────────────────────────────────

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

// ── StringInterner ─────────────────────────────────────────────────

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
