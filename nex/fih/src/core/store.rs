// ── FihStorage — unified FIH storage over FileIo ───────────────────────
//
// FihStorage is an execution unit. Each instance runs on a single thread
// with exclusive ownership of its in-memory state (FihCoord indices,
// entity stores, pending buffer) and I/O channel. There is no internal
// concurrency: no Mutex, no RwLock, no thread pool. Scaling happens
// through physical instance replication (multiple FihStorage instances,
// each independent), not through internal sharding.
//
// FihStorage does NOT implement sync storage traits (FactCapable,
// IntentCapable, etc.). All public methods are async. This is not a
// WASM concession — it is a consequence of storage being inherently
// I/O-bound and FihStorage being a single-threaded execution unit.
// Blocking on I/O would stall the sole thread and starve all pending
// operations. Sync callers use futures_executor::block_on externally
// (see FihBlackboard for a convenience wrapper on native platforms).
//
// Interior mutability uses RefCell, not Mutex, because there is no
// concurrent access within an instance. This is the simplest correct
// implementation for a single-owner model. If thread-safe access is
// needed, the caller wraps the instance in Arc<Mutex<FihStorage>> —
// that is an external composition, not an internal requirement.
//
// No static or static mut state exists in FihStorage except fixed
// constants. Every resource is owned by the instance. Spawning a new
// instance is purely a construction operation with no global side
// effects.
//
// Design invariants:
//   - enqueue_content() enqueues WriteOps via pending, never calls
//     io.write() directly
//   - read_state() loads blob Content from IO via blob_hash
//   - conclude_intent() passes real to_fact/concluded_at to try_conclude()
//   - all timestamps flow through Now trait, never SystemTime::now() directly
//   - no sync trait on FihStorage (async-only)
//   - no static mutable state

use std::ops::Range;

use sha2::Digest;

use crate::{
    BlackboardError, BoardState, Content, CoordId, Fact, FihHash, FlushCursor, FlushResult, Hint,
    Intent, PartitionData, StateFilter,
};
use nex_core::Now;

use crate::core::entity_store::{CoordEntityStore, EntityStore};
use crate::core::index::Cell2;
use crate::core::record::{ContentMeta, FactRecord, HintRecord, IntentRecord, IntentStatus};
use crate::io::file_io::{FileIo, WriteOp, default_apply_batch};
use crate::semantic::record::{Query, RecordLoad};
use std::collections::HashMap;

/// Chain entry format: serialized by flush_since for delta chain files.
/// Named struct avoids postcard tuple field ordering ambiguity with empty vecs.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ChainEntry {
    pub prev_cursor: u64,
    pub records_flushed: u64,
    pub facts: Vec<FactRecord>,
    pub intents: Vec<IntentRecord>,
}

/// Unified in-memory record enum for single CoordSpaceN<19> store.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub enum Record {
    Fact {
        content: Content,
        content_hash: FihHash,
        origin: String,
        creator: String,
        submitted_at: u64,
    },
    Intent {
        from_facts: Vec<String>,
        description_hash: String,
        creator: String,
        status: IntentStatus,
        created_at: u64,
    },
    Hint {
        content: String,
        creator: String,
        submitted_at: u64,
    },
}

/// Encode up to 112 bits of a FihHash into 7 contiguous Coord axes.
///
/// Each axis holds log₂(11172) ≈ 13.45 bits. 7 axes × 13.45 ≈ 94 effective bits.
/// Uses bytes [0..14) of the hash (112 bits), paired into 7 u16 values,
/// each reduced modulo 11172. Zero hash (all bytes 0) keeps axes at zero.
fn encode_hash_into_axes(hash: &FihHash, axes: &mut [tagma_core::Coord; 7]) {
    use tagma_core::Coord;
    for (i, axis) in axes.iter_mut().enumerate() {
        let lo = hash.0.get(i * 2).copied().unwrap_or(0) as u16;
        let hi = hash.0.get(i * 2 + 1).copied().unwrap_or(0) as u16;
        let idx = (u16::from_le_bytes([lo as u8, hi as u8])) % 11172;
        *axis = Coord::new(idx).unwrap_or_else(|| Coord::new(0).unwrap());
    }
}

/// Build a CoordPath<19> from entity fields (for unified Record store).
///
/// `content_hash` is encoded into axes [12-18] (94 effective bits of content
/// addressing). Pass `&FihHash([0u8; 32])` for Intent/Hint records that have
/// no content hash.
pub fn record_to_path(
    entity: u16,
    origin: &str,
    creator: &str,
    status: u16,
    id_str: &str,
    ts_ns: u64,
    content_hash: &FihHash,
) -> tagma_core::CoordPath<19> {
    use tagma_core::{Coord, CoordPath};
    let mk = |v: u16| Coord::new(v % 11172).unwrap();
    let time_hi = (ts_ns / 86_400_000_000_000) as u16;
    let time_lo = (ts_ns % 86_400_000_000_000 / 1_000_000_000) as u16; // seconds, not ns
    // Use hash-based coord for origin/creator (matches make_record_path convention)
    fn hash_str(s: &str) -> u16 {
        use sha2::Digest;
        let mut h = sha2::Sha256::new();
        h.update(s.as_bytes());
        let hash = h.finalize();
        u16::from_le_bytes([hash[0], hash[1]]) % 11172
    }
    let origin_v = hash_str(origin);
    let creator_v = hash_str(creator);
    let mut coords = [Coord::new(0).unwrap(); 19];
    coords[0] = mk(time_hi);
    coords[1] = mk(time_lo);
    coords[2] = mk(entity);
    coords[3] = mk(origin_v);
    coords[4] = mk(creator_v);
    coords[5] = mk(status);
    // [6-11]: identity from CoordId<6> coordinates directly (round-trip safe)
    let cid: CoordId = CoordId::from_string(id_str);
    let cid_coords: &[tagma_core::Coord; 6] = cid.0.coords();
    coords[6..12].copy_from_slice(&cid_coords[..6]);
    // [12-18]: content hash fingerprint (94 effective bits)
    let mut hash_axes = [Coord::new(0).unwrap(); 7];
    encode_hash_into_axes(content_hash, &mut hash_axes);
    coords[12..19].copy_from_slice(&hash_axes);
    CoordPath::new(coords)
}

/// Extract id string from CoordPath<19> identity coords [6..11] (CoordId<6> coordinates).
fn path_to_id_str(path: &tagma_core::CoordPath<19>) -> String {
    use tagma_core::{Coord, CoordPath};
    let mut coords = [Coord::new(0).unwrap(); 6];
    coords.copy_from_slice(&path.coords()[6..12]);
    let cid = CoordId::<6>(CoordPath::new(coords));
    cid.to_string()
}

/// Unified FIH storage backended by an abstract IO layer.
///
/// All FIH trait methods are sync. They enqueue WriteOps into a buffer
/// for batch commit by the outer FihSession layer.
/// IO-bound operations (flush_pending, rebuild_cache) are async.
pub struct FihStorage<I: FileIo> {
    pub io: I,
    project_id: String,
    clock: Box<dyn Now + Send + Sync>,
    /// When true, every write operation also flushes pending ops to IO
    /// immediately, ensuring durability at the cost of batching.
    #[expect(dead_code)]
    auto_flush: bool,
    // In-memory stores: rebuilt from IO on hydrate, kept in sync for reads.
    pub fact_store: CoordEntityStore<6, FactRecord>,
    pub intent_store: CoordEntityStore<6, IntentRecord>,
    pub hint_store: CoordEntityStore<6, HintRecord>,
    store: Cell2<tagma_core::CoordSpaceN<19, Record>>,
    pub fact_records: Cell2<HashMap<String, FactRecord>>,
    pub intent_records: Cell2<HashMap<String, IntentRecord>>,
    pub hint_records: Cell2<HashMap<String, HintRecord>>,
    pub facts_by_creator: Cell2<HashMap<String, Vec<String>>>,
    pub facts_by_origin: Cell2<HashMap<String, Vec<String>>>,
    // Semantic stores (for similarity search).
    semantic_stores: Cell2<Vec<crate::semantic::DynSemanticStore>>,
    /// Counter for assigning semantic IDs to facts incrementally.
    semantic_id_counter: Cell2<u32>,
    // Pending writes (for FihSession coordination).
    pub(crate) pending: Cell2<Vec<WriteOp>>,
}

impl<I: FileIo> FihStorage<I> {
    pub fn new(io: I, project_id: &str) -> Self {
        Self::with_clock(io, project_id, Box::new(nex_core::SystemClock))
    }

    pub fn with_clock(io: I, project_id: &str, clock: Box<dyn Now + Send + Sync>) -> Self {
        Self::with_clock_and_memory(io, project_id, clock)
    }

    /// Create storage with auto-flush enabled. Every write operation
    /// immediately flushes pending ops to IO for durability.
    /// Useful for R2-backed or direct-write deployments.
    pub fn with_auto_flush(io: I, project_id: &str) -> Self {
        Self::with_all(io, project_id, Box::new(nex_core::SystemClock), true)
    }

    /// Full constructor with all options.
    pub fn with_all(
        io: I,
        project_id: &str,
        clock: Box<dyn Now + Send + Sync>,
        auto_flush: bool,
    ) -> Self {
        Self {
            io,
            project_id: project_id.to_string(),
            clock,
            auto_flush,
            fact_store: CoordEntityStore::<6, FactRecord>::new(),
            intent_store: CoordEntityStore::<6, IntentRecord>::new(),
            hint_store: CoordEntityStore::<6, HintRecord>::new(),
            store: Cell2::new(tagma_core::CoordSpaceN::new()),
            fact_records: Cell2::new(HashMap::new()),
            intent_records: Cell2::new(HashMap::new()),
            hint_records: Cell2::new(HashMap::new()),
            facts_by_creator: Cell2::new(HashMap::new()),
            facts_by_origin: Cell2::new(HashMap::new()),
            semantic_stores: Cell2::new(Vec::new()),
            semantic_id_counter: Cell2::new(0u32),
            pending: Cell2::new(Vec::new()),
        }
    }

    /// Create storage with in-memory state only (no auto-flush).
    pub fn with_clock_and_memory(
        io: I,
        project_id: &str,
        clock: Box<dyn Now + Send + Sync>,
    ) -> Self {
        Self {
            io,
            project_id: project_id.to_string(),
            clock,
            auto_flush: false,
            fact_store: CoordEntityStore::<6, FactRecord>::new(),
            intent_store: CoordEntityStore::<6, IntentRecord>::new(),
            hint_store: CoordEntityStore::<6, HintRecord>::new(),
            store: Cell2::new(tagma_core::CoordSpaceN::new()),
            fact_records: Cell2::new(HashMap::new()),
            intent_records: Cell2::new(HashMap::new()),
            hint_records: Cell2::new(HashMap::new()),
            facts_by_creator: Cell2::new(HashMap::new()),
            facts_by_origin: Cell2::new(HashMap::new()),
            semantic_stores: Cell2::new(Vec::new()),
            semantic_id_counter: Cell2::new(0u32),
            pending: Cell2::new(Vec::new()),
        }
    }

    /// Rebuild in-memory cache from IO storage.
    pub async fn rebuild_cache(&self) -> Result<(), String> {
        let fact_keys = self.io.list("facts/").await?;
        let mut facts: Vec<(String, FactRecord)> = Vec::new();
        for key in fact_keys {
            if let Some(bytes) = self.io.read(&key).await?
                && let Ok(record) = postcard::from_bytes::<FactRecord>(&bytes)
            {
                facts.push((record.id.clone(), record));
            }
        }

        let intent_keys = self.io.list("intents/").await?;
        let mut intents: Vec<(String, IntentRecord)> = Vec::new();
        for key in intent_keys {
            if let Some(bytes) = self.io.read(&key).await?
                && let Ok(record) = postcard::from_bytes::<IntentRecord>(&bytes)
            {
                intents.push((record.id.clone(), record));
            }
        }

        let hint_keys = self.io.list("hints/").await?;
        let mut hints: Vec<(String, HintRecord)> = Vec::new();
        for key in hint_keys {
            if let Some(bytes) = self.io.read(&key).await?
                && let Ok(record) = postcard::from_bytes::<HintRecord>(&bytes)
            {
                hints.push((record.id.clone(), record));
            }
        }

        self.fact_store.replace_from(facts).await;
        self.intent_store.replace_from(intents).await;
        self.hint_store.replace_from(hints).await;

        Ok(())
    }

    /// Flush pending writes to IO.
    ///
    /// On apply failure the batch is re-queued at the front of `pending`
    /// so a later flush retries it. Write and Delete are idempotent, so
    /// re-applying an already-applied prefix is safe.
    pub async fn flush_pending(&self) -> Result<(), String> {
        let ops = {
            let mut pending = self.pending.borrow_mut();
            if pending.is_empty() {
                return Ok(());
            }
            std::mem::take(&mut *pending)
        };
        if let Err(e) = default_apply_batch(&self.io, &ops).await {
            let mut pending = self.pending.borrow_mut();
            pending.splice(0..0, ops);
            return Err(e);
        }
        Ok(())
    }

    /// Read and decode durable records under `prefix` whose timestamp
    /// passes `filter`, in io order. Undecodable files are skipped: a
    /// prefix may hold artifacts that are not records, and a future
    /// layout change should not fail the scan.
    async fn scan_durable_records<R>(
        &self,
        prefix: &str,
        filter: impl Fn(&R) -> bool,
    ) -> Result<Vec<R>, String>
    where
        R: serde::de::DeserializeOwned,
    {
        let keys = self.io.list(prefix).await?;
        let mut out = Vec::new();
        for key in keys {
            if let Some(bytes) = self.io.read(&key).await?
                && let Ok(record) = postcard::from_bytes::<R>(&bytes)
                && filter(&record)
            {
                out.push(record);
            }
        }
        Ok(out)
    }

    /// Rebuild semantic stores from fact_store after rebuild_cache.
    pub async fn rebuild_semantic(&self) -> Result<(), String> {
        // Snapshot: take stores atomically, work on them, then put back.
        let mut stores = std::mem::take(&mut *self.semantic_stores.borrow_mut());
        if stores.is_empty() {
            return Ok(());
        }

        let facts = self.fact_store.values().await;
        struct TextRecord {
            text: String,
        }
        impl crate::semantic::record::RecordLoad for TextRecord {
            fn content(&self, _id: u32) -> Option<Vec<u8>> {
                Some(self.text.as_bytes().to_vec())
            }
            fn features(&self, _id: u32) -> Option<Vec<f32>> {
                None
            }
        }

        for (i, r) in facts.iter().enumerate() {
            let content = load_blob(&self.io, &r.blob_hash).await;
            if content.data.is_empty() {
                continue;
            }
            let text = String::from_utf8_lossy(&content.data).to_string();
            if text.trim().is_empty() {
                continue;
            }
            let load = TextRecord { text };
            for store in stores.iter_mut() {
                let _ = store.insert(i as u32, &load).await;
            }
        }

        // Put stores back
        self.semantic_stores.borrow_mut().extend(stores);
        Ok(())
    }

    /// Register a semantic store for auto-indexing on fact submission.
    pub fn register_semantic_store(&self, store: crate::semantic::DynSemanticStore) {
        self.semantic_stores.borrow_mut().push(store);
    }

    /// Access the semantic stores list (for downcasting to concrete types).
    pub fn semantic_stores(
        &self,
    ) -> impl std::ops::Deref<Target = Vec<crate::semantic::DynSemanticStore>> {
        self.semantic_stores.borrow()
    }

    /// Search semantic stores with the given query.
    ///
    /// Uses take/extend pattern to avoid holding a non-Send MutexGuard
    /// across an async boundary.
    pub async fn semantic_search(
        &self,
        query: &dyn Query,
        top_k: usize,
    ) -> Result<Vec<(u32, f32)>, String> {
        let mut stores = std::mem::take(&mut *self.semantic_stores.borrow_mut());
        if stores.is_empty() {
            self.semantic_stores.borrow_mut().extend(stores);
            return Err("no semantic stores configured".into());
        }
        let mut results = Vec::new();
        for store in stores.iter_mut() {
            if let Ok(mut r) = store.search(query, top_k).await {
                results.append(&mut r);
            }
        }
        self.semantic_stores.borrow_mut().extend(stores);
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(top_k);
        Ok(results)
    }

    /// Insert a record into semantic stores.
    ///
    /// Uses take/extend pattern (not borrow_mut across await) to avoid
    /// holding a non-Send MutexGuard across an async boundary.
    pub async fn semantic_insert(&self, id: u32, load: &dyn RecordLoad) -> Result<(), String> {
        let mut stores = std::mem::take(&mut *self.semantic_stores.borrow_mut());
        if stores.is_empty() {
            self.semantic_stores.borrow_mut().extend(stores);
            return Err("no semantic stores configured".into());
        }
        let mut last_err: Option<String> = None;
        for store in stores.iter_mut() {
            if let Err(e) = store.insert(id, load).await {
                last_err = Some(e);
            }
        }
        self.semantic_stores.borrow_mut().extend(stores);
        if let Some(e) = last_err {
            Err(e)
        } else {
            Ok(())
        }
    }

    /// Query intents that reference a given fact.
    /// The fact_id is normalized via CoordId::from_string to match the
    /// canonical CoordId format stored in IntentRecord.from_facts.
    pub fn intents_by_fact(&self, fact_id: &str) -> Vec<String> {
        let normalized = crate::CoordId::from_string(fact_id).to_string();
        let intent_records: Vec<IntentRecord> =
            futures_executor::block_on(self.intent_store.values());
        intent_records
            .iter()
            .filter(|r| r.from_facts.iter().any(|f| f == &normalized))
            .map(|r| r.id.clone())
            .collect()
    }

    /// Direct record placement (for special cases like nex-calc).
    /// Skips blob enqueue and fast-path index maintenance.
    pub fn place_record(&self, path: &tagma_core::CoordPath<19>, record: Record) {
        self.store.borrow_mut().place_path(path, record);
    }

    /// Direct record removal (for special cases like nex-calc).
    pub fn vacate_record(&self, path: &tagma_core::CoordPath<19>) {
        self.store.borrow_mut().vacate_path(path);
    }

    /// Resolve a semantic index back to its ID string.
    pub fn resolve_semantic_idx(&self, idx: u32) -> String {
        let records = futures_executor::block_on(self.fact_store.values());
        records
            .get(idx as usize)
            .map(|r| r.id.clone())
            .unwrap_or_default()
    }

    /// Check if a fact with the given ID exists (fast-path: fact_records HashMap).
    pub fn fact_exists(&self, id: &str) -> bool {
        self.fact_records.borrow().contains_key(id)
            || self.store.borrow().iter_tree().any(|(path, rec)| {
                matches!(rec, Record::Fact { .. }) && path_to_id_str(&path) == id
            })
    }

    /// Check if an intent with the given ID exists (fast-path: intent_records HashMap).
    pub fn intent_exists(&self, id: &str) -> bool {
        self.intent_records.borrow().contains_key(id)
            || self.store.borrow().iter_tree().any(|(path, rec)| {
                matches!(rec, Record::Intent { .. }) && path_to_id_str(&path) == id
            })
    }

    /// Check if a hint with the given ID exists (fast-path: hint_records HashMap).
    pub fn hint_exists(&self, id: &str) -> bool {
        self.hint_records.borrow().contains_key(id)
            || self.store.borrow().iter_tree().any(|(path, rec)| {
                matches!(rec, Record::Hint { .. }) && path_to_id_str(&path) == id
            })
    }

    /// Returns all fact IDs (fast-path: HashMap keys; fallback: unified store).
    pub fn all_fact_ids(&self) -> Vec<String> {
        let recs = self.fact_records.borrow();
        if !recs.is_empty() {
            return recs.keys().cloned().collect();
        }
        // Fallback: unified store (for direct writers like nex-calc)
        self.store
            .borrow()
            .iter_tree()
            .filter_map(|(path, rec)| {
                if matches!(rec, Record::Fact { .. }) {
                    Some(path_to_id_str(&path))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Returns all intent IDs (fast-path: HashMap keys; fallback: unified store).
    pub fn all_intent_ids(&self) -> Vec<String> {
        let recs = self.intent_records.borrow();
        if !recs.is_empty() {
            return recs.keys().cloned().collect();
        }
        self.store
            .borrow()
            .iter_tree()
            .filter_map(|(path, rec)| {
                if matches!(rec, Record::Intent { .. }) {
                    Some(path_to_id_str(&path))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Returns all hint IDs (fast-path: HashMap keys; fallback: unified store).
    pub fn all_hint_ids(&self) -> Vec<String> {
        let recs = self.hint_records.borrow();
        if !recs.is_empty() {
            return recs.keys().cloned().collect();
        }
        self.store
            .borrow()
            .iter_tree()
            .filter_map(|(path, rec)| {
                if matches!(rec, Record::Hint { .. }) {
                    Some(path_to_id_str(&path))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get a fact by its ID (fast-path: fact_records HashMap).
    pub fn get_fact_by_id(&self, id: &str) -> Option<(Content, FihHash, String, String)> {
        if let Some(r) = self.fact_records.borrow().get(id) {
            let hex = &r.blob_hash;
            let mut bytes = [0u8; 32];
            for (i, c) in hex.as_bytes().chunks(2).enumerate() {
                let s = unsafe { std::str::from_utf8_unchecked(c) };
                bytes[i] = u8::from_str_radix(s, 16).unwrap_or(0);
            }
            return Some((
                Content {
                    mime_type: "application/octet-stream".into(),
                    data: Vec::new(),
                },
                FihHash(bytes),
                r.origin.clone(),
                r.creator.clone(),
            ));
        }
        // Fallback: unified store (for direct store writers like nex-calc)
        for (_path, rec) in self.store.borrow().iter_tree() {
            if let Record::Fact {
                content,
                content_hash,
                origin,
                creator,
                ..
            } = rec
                && path_to_id_str(&_path) == id
            {
                return Some((
                    content.clone(),
                    *content_hash,
                    origin.clone(),
                    creator.clone(),
                ));
            }
        }
        None
    }

    /// Get an intent by its ID.
    pub fn get_intent_by_id(
        &self,
        id: &str,
    ) -> Option<(Vec<String>, String, String, IntentStatus, u64)> {
        if let Some(r) = self.intent_records.borrow().get(id) {
            return Some((
                r.from_facts.clone(),
                r.description_hash.clone(),
                r.creator.clone(),
                r.status.clone(),
                r.created_at,
            ));
        }
        for (_path, rec) in self.store.borrow().iter_tree() {
            if let Record::Intent {
                from_facts,
                description_hash,
                creator,
                status,
                created_at,
            } = rec
                && path_to_id_str(&_path) == id
            {
                return Some((
                    from_facts.clone(),
                    description_hash.clone(),
                    creator.clone(),
                    status.clone(),
                    *created_at,
                ));
            }
        }
        None
    }

    /// Get a hint by its ID.
    pub fn get_hint_by_id(&self, id: &str) -> Option<(String, String, u64)> {
        if let Some(r) = self.hint_records.borrow().get(id) {
            return Some((r.content.clone(), r.creator.clone(), r.submitted_at));
        }
        for (_path, rec) in self.store.borrow().iter_tree() {
            if let Record::Hint {
                content,
                creator,
                submitted_at,
            } = rec
                && path_to_id_str(&_path) == id
            {
                return Some((content.clone(), creator.clone(), *submitted_at));
            }
        }
        None
    }

    /// Load blob content from pending writes. No IO fallback — FIH is
    /// append-only and content is stored alongside facts for reconstruction.
    /// Content blob data is only materialized during export/flush;
    /// read_state returns empty content for non-pending blobs.
    fn load_content(&self, blob_hash: &str, default_mime: &str) -> Content {
        let blob_path = format!("blob/{}.bin", blob_hash);
        let meta_path = format!("blob/{}.bin.meta", blob_hash);

        // Check pending writes for blob data and mime
        let pending = self.pending.borrow();
        let mut blob_data = None;
        let mut mime = None;
        for op in pending.iter() {
            match op {
                WriteOp::Write { path, data } if *path == blob_path => {
                    blob_data = Some(data.clone());
                }
                WriteOp::Write { path, data } if *path == meta_path => {
                    if let Ok(meta) = postcard::from_bytes::<ContentMeta>(data) {
                        mime = Some(meta.mime_type);
                    }
                }
                _ => {}
            }
        }
        drop(pending);

        if let Some(data) = blob_data {
            return Content {
                mime_type: mime.unwrap_or_else(|| default_mime.to_string()),
                data,
            };
        }

        // No data in pending — return empty content.
        // The async path (`AsyncStorageRead::read_state`) calls `load_blob`
        // directly to fetch from IO. The sync path only has access to
        // in-memory caches; after `flush_pending` + `rebuild_cache` the
        // content lives in IO but `load_content` cannot reach it without
        // performing synchronous IO, which is intentionally avoided.
        Content {
            mime_type: default_mime.to_string(),
            data: Vec::new(),
        }
    }
}

/// Convert IntentStatus to a simple string key for fast-path status lookup.
fn simple_status_key(status: &IntentStatus) -> &'static str {
    match status {
        IntentStatus::Submitted => "submitted",
        IntentStatus::Claimed { .. } => "claimed",
        IntentStatus::Concluded { .. } => "concluded",
    }
}

fn content_hash(data: &[u8]) -> String {
    // SHA-256 content hash. WASM-compatible (sha2 crate works on wasm32).
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

/// Load a content blob from IO by hash. Returns empty Content if not found.
async fn load_blob(io: &impl FileIo, blob_hash: &str) -> Content {
    if blob_hash.is_empty() {
        return Content {
            mime_type: "application/json".into(),
            data: Vec::new(),
        };
    }
    let key = format!("blob/{}.bin", blob_hash);
    let meta_key = format!("blob/{}.bin.meta", blob_hash);

    let mime = io.read(&meta_key).await.ok().flatten().and_then(|bytes| {
        postcard::from_bytes::<ContentMeta>(&bytes)
            .ok()
            .map(|m| m.mime_type)
    });

    match io.read(&key).await {
        Ok(Some(data)) => Content {
            mime_type: mime.unwrap_or_else(|| "application/json".into()),
            data,
        },
        _ => Content {
            mime_type: mime.unwrap_or_else(|| "application/json".into()),
            data: Vec::new(),
        },
    }
}

// ── AsyncStorageRead ───────────────────────────────────────────────────────

impl<I: FileIo> crate::AsyncStorageRead for FihStorage<I> {
    fn project_id(&self) -> &str {
        &self.project_id
    }

    async fn read_state(&self) -> BoardState {
        // Flush any pending writes so IO reflects the latest state. The
        // signature has no error channel, so a failed flush is logged
        // instead of silently returning stale state.
        if let Err(e) = self.flush_pending().await {
            log::warn!("read_state: flush pending failed: {e}");
        }

        // Direct async IO: list + read from backing store, no block_on.
        let mut facts = Vec::new();
        if let Ok(keys) = self.io.list("facts/").await {
            for key in &keys {
                if let Ok(Some(bytes)) = self.io.read(key).await
                    && let Ok(r) = postcard::from_bytes::<FactRecord>(&bytes)
                {
                    let content = load_blob(&self.io, &r.blob_hash).await;
                    let content_hash = {
                        let mut h = sha2::Sha256::new();
                        h.update(&content.data);
                        FihHash(h.finalize().into())
                    };
                    facts.push(Fact {
                        id: CoordId::from_string(&r.id),
                        content_hash,
                        origin: r.origin.clone(),
                        content,
                        creator: r.creator.clone(),
                    });
                }
            }
        }

        let mut intents = Vec::new();
        if let Ok(keys) = self.io.list("intents/").await {
            for key in &keys {
                if let Ok(Some(bytes)) = self.io.read(key).await
                    && let Ok(r) = postcard::from_bytes::<IntentRecord>(&bytes)
                {
                    intents.push(Intent {
                        id: CoordId::from_string(&r.id),
                        from_facts: r
                            .from_facts
                            .iter()
                            .map(|s| CoordId::from_string(s))
                            .collect(),
                        description: {
                            if r.description_hash.is_empty() {
                                r.id.clone()
                            } else {
                                let c = load_blob(&self.io, &r.description_hash).await;
                                String::from_utf8_lossy(&c.data).to_string()
                            }
                        },
                        creator: r.creator.clone(),
                        worker: match &r.status {
                            IntentStatus::Claimed { worker, .. }
                            | IntentStatus::Concluded { worker, .. } => Some(worker.clone()),
                            IntentStatus::Submitted => None,
                        },
                        to_fact_id: match &r.status {
                            IntentStatus::Concluded { to_fact, .. } => {
                                Some(CoordId::from_string(to_fact))
                            }
                            _ => None,
                        },
                        last_heartbeat_at: match &r.status {
                            IntentStatus::Claimed {
                                last_heartbeat_at, ..
                            } => Some(*last_heartbeat_at),
                            _ => None,
                        },
                        created_at: Some(r.created_at),
                        is_concluded: matches!(&r.status, IntentStatus::Concluded { .. }),
                        concluded_at: match &r.status {
                            IntentStatus::Concluded { concluded_at, .. } => Some(*concluded_at),
                            _ => None,
                        },
                    });
                }
            }
        }

        let mut hints = Vec::new();
        if let Ok(keys) = self.io.list("hints/").await {
            for key in &keys {
                if let Ok(Some(bytes)) = self.io.read(key).await
                    && let Ok(r) = postcard::from_bytes::<HintRecord>(&bytes)
                {
                    hints.push(Hint {
                        id: CoordId::from_string(&r.id),
                        content: r.content.clone(),
                        creator: r.creator.clone(),
                    });
                }
            }
        }

        BoardState {
            facts,
            intents,
            hints,
        }
    }
}

// ── AsyncFactCapable ───────────────────────────────────────────────────────

impl<I: FileIo> crate::AsyncFactCapable for FihStorage<I> {
    async fn submit_fact(&self, fact: &Fact) -> Result<CoordId, BlackboardError> {
        // content_hash is SHA-256 already computed by Fact::new — use directly.
        let blob_hash = fact.content_hash.to_string();

        // Enqueue blob data (mime from content).
        let blob_path = format!("blob/{blob_hash}.bin");
        self.pending.borrow_mut().push(WriteOp::Write {
            path: blob_path,
            data: fact.content.data.clone(),
        });
        let meta = ContentMeta {
            mime_type: fact.content.mime_type.clone(),
            size: fact.content.data.len() as u64,
        };
        let meta_bytes =
            postcard::to_allocvec(&meta).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.pending.borrow_mut().push(WriteOp::Write {
            path: format!("blob/{blob_hash}.bin.meta"),
            data: meta_bytes,
        });

        let record = FactRecord::from_model(fact, blob_hash, self.clock.now_nanos());
        let bytes =
            postcard::to_allocvec(&record).map_err(|e| BlackboardError::Internal(e.to_string()))?;

        let op = WriteOp::Write {
            path: record.key(),
            data: bytes,
        };

        // Update in-memory cache immediately for subsequent reads
        self.fact_store
            .insert(record.id.clone(), record.clone())
            .await;
        self.fact_records
            .borrow_mut()
            .insert(record.id.clone(), record);
        self.facts_by_creator
            .borrow_mut()
            .entry(fact.creator.clone())
            .or_default()
            .push(fact.id.to_string());
        self.facts_by_origin
            .borrow_mut()
            .entry(fact.origin.clone())
            .or_default()
            .push(fact.id.to_string());
        self.pending.borrow_mut().push(op);

        // Auto-index into semantic stores (skip conclusion facts to reduce noise)
        if !fact.origin.starts_with("conclusion:") {
            let semantic_idx = {
                let mut ctr = self.semantic_id_counter.borrow_mut();
                let idx = *ctr;
                *ctr += 1;
                idx
            };
            let text = String::from_utf8_lossy(&fact.content.data).to_string();
            struct FactTextRecord {
                text: String,
            }
            impl crate::semantic::record::RecordLoad for FactTextRecord {
                fn content(&self, _id: u32) -> Option<Vec<u8>> {
                    Some(self.text.as_bytes().to_vec())
                }
                fn features(&self, _id: u32) -> Option<Vec<f32>> {
                    None
                }
            }
            self.semantic_insert(semantic_idx, &FactTextRecord { text })
                .await
                .ok();
        }

        Ok(fact.id)
    }
}

// ── AsyncHintCapable ───────────────────────────────────────────────────────

impl<I: FileIo> crate::AsyncHintCapable for FihStorage<I> {
    async fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        let record = super::record::HintRecord {
            id: hint.id.to_string(),
            content: hint.content.clone(),
            creator: hint.creator.clone(),
            submitted_at: self.clock.now_secs(),
            ttl_secs: None,
        };
        let bytes =
            postcard::to_allocvec(&record).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        let op = WriteOp::Write {
            path: record.key(),
            data: bytes,
        };
        self.hint_store
            .insert(record.id.clone(), record.clone())
            .await;
        self.hint_records
            .borrow_mut()
            .insert(record.id.clone(), record);
        self.pending.borrow_mut().push(op);
        Ok(())
    }
}

// ── AsyncIntentCapable ─────────────────────────────────────────────────────

impl<I: FileIo> crate::AsyncIntentCapable for FihStorage<I> {
    async fn submit_intent(&self, intent: &Intent) -> Result<CoordId, BlackboardError> {
        if intent.from_facts.is_empty() {
            return Err(BlackboardError::Forbidden(
                "intent must reference at least one fact".into(),
            ));
        }
        for fid in &intent.from_facts {
            let fid_str = fid.to_string();
            if !self.fact_store.contains_key(&fid_str).await {
                return Err(BlackboardError::NotFound(format!(
                    "Fact {fid_str} not found"
                )));
            }
        }

        // Store description as a blob if non-empty
        let description_hash = if !intent.description.is_empty() {
            let desc_bytes = intent.description.as_bytes();
            let hash = content_hash(desc_bytes);
            let meta = super::record::ContentMeta {
                mime_type: "text/plain".into(),
                size: desc_bytes.len() as u64,
            };
            let meta_bytes = postcard::to_allocvec(&meta).unwrap_or_default();
            self.pending.borrow_mut().push(WriteOp::Write {
                path: format!("blob/{hash}.bin"),
                data: desc_bytes.to_vec(),
            });
            self.pending.borrow_mut().push(WriteOp::Write {
                path: format!("blob/{hash}.bin.meta"),
                data: meta_bytes,
            });
            hash
        } else {
            String::new()
        };

        let record = super::record::IntentRecord {
            id: intent.id.to_string(),
            from_facts: intent.from_facts.iter().map(|f| f.to_string()).collect(),
            description_hash,
            creator: intent.creator.clone(),
            status: super::record::IntentStatus::Submitted,
            created_at: self.clock.now_secs(),
        };
        let bytes =
            postcard::to_allocvec(&record).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        let op = WriteOp::Write {
            path: record.key(),
            data: bytes,
        };

        self.intent_store
            .insert(record.id.clone(), record.clone())
            .await;
        self.intent_records
            .borrow_mut()
            .insert(record.id.clone(), record);
        self.pending.borrow_mut().push(op);
        Ok(intent.id)
    }

    async fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.flush_pending()
            .await
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        let normalized = CoordId::from_string(intent_id).to_string();
        let key = format!("intents/i_{}.intent", normalized);
        let bytes = self
            .io
            .read(&key)
            .await
            .map_err(BlackboardError::Internal)?
            .ok_or_else(|| BlackboardError::NotFound(format!("Intent {intent_id} not found")))?;
        let mut record = postcard::from_bytes::<IntentRecord>(&bytes)
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;

        let now = self.clock.now_secs();
        let new_status = record.status.try_claim(agent, now).map_err(|e| {
            if e.starts_with("already claimed") {
                BlackboardError::Conflict(e)
            } else {
                BlackboardError::Internal(e)
            }
        })?;
        record.status = new_status;

        let bytes =
            postcard::to_allocvec(&record).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.pending.borrow_mut().push(WriteOp::Write {
            path: key,
            data: bytes,
        });
        self.flush_pending()
            .await
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.intent_store.insert(normalized.clone(), record).await;
        Ok(())
    }

    async fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.flush_pending()
            .await
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        let normalized = CoordId::from_string(intent_id).to_string();
        let key = format!("intents/i_{}.intent", normalized);
        let bytes = self
            .io
            .read(&key)
            .await
            .map_err(BlackboardError::Internal)?
            .ok_or_else(|| BlackboardError::NotFound(format!("Intent {intent_id} not found")))?;
        let mut record = postcard::from_bytes::<IntentRecord>(&bytes)
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;

        let now = self.clock.now_secs();
        let new_status = record.status.try_heartbeat(agent, now).map_err(|e| {
            if e.contains("not") {
                BlackboardError::Conflict(e)
            } else {
                BlackboardError::Internal(e)
            }
        })?;
        record.status = new_status;

        let bytes =
            postcard::to_allocvec(&record).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.pending.borrow_mut().push(WriteOp::Write {
            path: key.clone(),
            data: bytes,
        });
        self.flush_pending()
            .await
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.intent_store
            .insert(intent_id.to_string(), record)
            .await;
        Ok(())
    }

    async fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.flush_pending()
            .await
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        let normalized = CoordId::from_string(intent_id).to_string();
        let key = format!("intents/i_{}.intent", normalized);
        let bytes = self
            .io
            .read(&key)
            .await
            .map_err(BlackboardError::Internal)?
            .ok_or_else(|| BlackboardError::NotFound(format!("Intent {intent_id} not found")))?;
        let mut record = postcard::from_bytes::<IntentRecord>(&bytes)
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;

        match &record.status {
            IntentStatus::Claimed { worker, .. } if worker == agent => {
                record.status = IntentStatus::Submitted;
            }
            IntentStatus::Claimed { worker, .. } => {
                return Err(BlackboardError::Forbidden(format!(
                    "Intent {intent_id} claimed by {worker}, not {agent}"
                )));
            }
            IntentStatus::Submitted => return Ok(()),
            IntentStatus::Concluded { .. } => {
                return Err(BlackboardError::NotFound(format!(
                    "Intent {intent_id} already concluded"
                )));
            }
        }

        let bytes =
            postcard::to_allocvec(&record).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.pending.borrow_mut().push(WriteOp::Write {
            path: key,
            data: bytes,
        });
        self.flush_pending()
            .await
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.intent_store
            .insert(CoordId::from_string(intent_id).to_string(), record)
            .await;
        Ok(())
    }

    async fn conclude_intent(
        &self,
        intent_id: &str,
        result: &str,
    ) -> Result<Fact, BlackboardError> {
        self.flush_pending()
            .await
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        let normalized = CoordId::from_string(intent_id).to_string();
        let key = format!("intents/i_{}.intent", normalized);
        let bytes = self
            .io
            .read(&key)
            .await
            .map_err(BlackboardError::Internal)?
            .ok_or_else(|| BlackboardError::NotFound(format!("Intent {intent_id} not found")))?;
        let mut record = postcard::from_bytes::<IntentRecord>(&bytes)
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;

        let worker = match &record.status {
            IntentStatus::Claimed { worker, .. } => worker.clone(),
            IntentStatus::Submitted => return Err(BlackboardError::Internal("not claimed".into())),
            IntentStatus::Concluded { .. } => {
                return Err(BlackboardError::Internal("already concluded".into()));
            }
        };

        let conclusion_id = format!("f_concl_{}", intent_id);
        let content_data = result.as_bytes().to_vec();
        let content_hash = {
            let mut h = sha2::Sha256::new();
            h.update(&content_data);
            FihHash(h.finalize().into())
        };
        let new_fact = Fact {
            id: CoordId::from_string(&conclusion_id),
            content_hash,
            origin: format!("conclusion:{}", intent_id),
            content: Content {
                mime_type: "text/plain".into(),
                data: result.as_bytes().to_vec(),
            },
            creator: worker.clone(),
        };

        let now_ns = self.clock.now_nanos();
        record.status = record
            .status
            .try_conclude(&conclusion_id, now_ns)
            .map_err(BlackboardError::Internal)?;

        // Write conclusion fact and updated intent via pending buffer.
        let fact_rec = FactRecord::from_model(&new_fact, String::new(), 0);
        let fact_bytes = postcard::to_allocvec(&fact_rec)
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.pending.borrow_mut().push(WriteOp::Write {
            path: fact_rec.key(),
            data: fact_bytes,
        });
        self.fact_store
            .insert(fact_rec.id.clone(), fact_rec.clone())
            .await;
        self.fact_records
            .borrow_mut()
            .insert(fact_rec.id.clone(), fact_rec);

        let intent_bytes =
            postcard::to_allocvec(&record).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.pending.borrow_mut().push(WriteOp::Write {
            path: key,
            data: intent_bytes,
        });
        self.flush_pending()
            .await
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.intent_store.insert(normalized.clone(), record).await;

        Ok(new_fact)
    }
}

// ── AsyncFilterCapable (in-memory filtering) ────────────────────────────

impl<I: FileIo> crate::AsyncFilterCapable for FihStorage<I> {
    async fn read_state_filtered(&self, filter: &StateFilter) -> BoardState {
        // Build blob lookup map once from pending writes (avoid O(N×P) scan).
        let blob_map: HashMap<String, (String, Vec<u8>)> = self
            .pending
            .borrow()
            .iter()
            .filter_map(|op| match op {
                WriteOp::Write { path, data }
                    if path.ends_with(".bin") && !path.ends_with(".bin.meta") =>
                {
                    let blob_hash = path
                        .strip_prefix("blob/")
                        .and_then(|p| p.strip_suffix(".bin"))?;
                    Some((blob_hash.to_string(), (String::new(), data.clone())))
                }
                WriteOp::Write { path, data } if path.ends_with(".bin.meta") => {
                    let blob_hash = path
                        .strip_prefix("blob/")
                        .and_then(|p| p.strip_suffix(".bin.meta"))?;
                    let meta = postcard::from_bytes::<ContentMeta>(data).ok()?;
                    Some((blob_hash.to_string(), (meta.mime_type, Vec::new())))
                }
                _ => None,
            })
            .collect();

        // Materialize content from pending blobs (no IO fallback for sync path).
        let load_content_fast = |blob_hash: &str, default_mime: &str| -> Content {
            if let Some((mime, data)) = blob_map.get(blob_hash)
                && !data.is_empty()
            {
                return Content {
                    mime_type: if mime.is_empty() {
                        default_mime.to_string()
                    } else {
                        mime.clone()
                    },
                    data: data.clone(),
                };
            }
            Content {
                mime_type: default_mime.to_string(),
                data: Vec::new(),
            }
        };

        // FAST PATH 1: single or AND filter via HashMap O(1)
        let fast_ids: Option<Vec<String>> = match (filter.creator.as_ref(), filter.origin.as_ref())
        {
            (Some(c), Some(o)) => {
                // AND: intersect creator + origin
                let by_c = self.facts_by_creator.borrow();
                let by_o = self.facts_by_origin.borrow();
                let c_ids = by_c.get(c);
                let o_ids = by_o.get(o);
                match (c_ids, o_ids) {
                    (Some(cv), Some(ov)) => {
                        if cv.len() <= ov.len() {
                            let o_set: std::collections::HashSet<&str> =
                                ov.iter().map(|s| s.as_str()).collect();
                            Some(
                                cv.iter()
                                    .filter(|id| o_set.contains(id.as_str()))
                                    .cloned()
                                    .collect(),
                            )
                        } else {
                            let c_set: std::collections::HashSet<&str> =
                                cv.iter().map(|s| s.as_str()).collect();
                            Some(
                                ov.iter()
                                    .filter(|id| c_set.contains(id.as_str()))
                                    .cloned()
                                    .collect(),
                            )
                        }
                    }
                    (Some(cv), None) => Some(cv.clone()),
                    (None, Some(ov)) => Some(ov.clone()),
                    (None, None) => Some(Vec::new()),
                }
            }
            (Some(c), None) => self.facts_by_creator.borrow().get(c).cloned(),
            (None, Some(o)) => self.facts_by_origin.borrow().get(o).cloned(),
            (None, None) => None,
        };
        if let Some(ids) = fast_ids {
            let recs = self.fact_records.borrow();
            let facts: Vec<Fact> = ids
                .iter()
                .filter_map(|id| {
                    recs.get(id).map(|r| Fact {
                        id: CoordId::from_string(&r.id),
                        origin: r.origin.clone(),
                        content_hash: {
                            let hex = &r.blob_hash;
                            let mut b = [0u8; 32];
                            for (i, c) in hex.as_bytes().chunks(2).enumerate() {
                                let s = unsafe { std::str::from_utf8_unchecked(c) };
                                b[i] = u8::from_str_radix(s, 16).unwrap_or(0);
                            }
                            FihHash(b)
                        },
                        content: load_content_fast(&r.blob_hash, "application/octet-stream"),
                        creator: r.creator.clone(),
                    })
                })
                .collect();
            let intents: Vec<Intent> = Vec::new();
            let hints: Vec<Hint> = Vec::new();
            let mut state = BoardState {
                facts,
                intents,
                hints,
            };
            if let Some(limit) = filter.limit {
                state.facts = state.facts.into_iter().take(limit).collect();
            }
            return state;
        }

        // FAST PATH 2: axis_hints enable O(subtree) iter_prefix query.
        if let Some(ref hints) = filter.axis_hints
            && hints.time_hi.is_some()
        {
            let matched = self.fact_store.query_prefix(hints).await;
            let facts: Vec<Fact> = matched
                .into_iter()
                .filter(|r| {
                    if let Some(ref origin) = filter.origin
                        && r.origin != *origin
                    {
                        return false;
                    }
                    if let Some(ref creator) = filter.creator
                        && r.creator != *creator
                    {
                        return false;
                    }
                    if let Some(ref since) = filter.since
                        && let Ok(ts) = since.parse::<u64>()
                        && r.submitted_at < ts
                    {
                        return false;
                    }
                    if let Some(ref until) = filter.until
                        && let Ok(ts) = until.parse::<u64>()
                        && r.submitted_at > ts
                    {
                        return false;
                    }
                    true
                })
                .map(|r| {
                    let content = load_content_fast(&r.blob_hash, "application/octet-stream");
                    Fact {
                        id: CoordId::from_string(&r.id),
                        origin: r.origin,
                        content_hash: {
                            let hex = &r.blob_hash;
                            let mut bytes = [0u8; 32];
                            for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
                                let s = unsafe { std::str::from_utf8_unchecked(chunk) };
                                bytes[i] = u8::from_str_radix(s, 16).unwrap_or(0);
                            }
                            FihHash(bytes)
                        },
                        content,
                        creator: r.creator,
                    }
                })
                .collect();
            let intents: Vec<Intent> = Vec::new();
            let hints: Vec<Hint> = Vec::new();
            let mut state = BoardState {
                facts,
                intents,
                hints,
            };
            if let Some(limit) = filter.limit {
                state.facts = state.facts.into_iter().take(limit).collect();
            }
            return state;
        }

        // FALLBACK: values() + string-based filter (no axis hints)
        let all_facts = self.fact_store.values().await;
        let all_intents = self.intent_store.values().await;
        let all_hints = self.hint_store.values().await;

        // Filter facts using record fields directly.
        let facts: Vec<Fact> = all_facts
            .into_iter()
            .filter(|r| {
                if let Some(ref origin) = filter.origin
                    && r.origin != *origin
                {
                    return false;
                }
                if let Some(ref creator) = filter.creator
                    && r.creator != *creator
                {
                    return false;
                }
                if let Some(ref since) = filter.since
                    && let Ok(ts) = since.parse::<u64>()
                    && r.submitted_at < ts
                {
                    return false;
                }
                if let Some(ref until) = filter.until
                    && let Ok(ts) = until.parse::<u64>()
                    && r.submitted_at > ts
                {
                    return false;
                }
                if let Some(ref ids) = filter.fact_ids {
                    let normalized: Vec<String> = ids
                        .iter()
                        .map(|id| CoordId::from_string(id).to_string())
                        .collect();
                    if !normalized.iter().any(|nid| nid == &r.id) {
                        return false;
                    }
                }
                true
            })
            .map(|r| {
                let content = load_content_fast(&r.blob_hash, "application/octet-stream");
                Fact {
                    id: CoordId::from_string(&r.id),
                    origin: r.origin,
                    content_hash: {
                        // blob_hash IS the hex-encoded content hash — parse directly.
                        let hex = &r.blob_hash;
                        let mut bytes = [0u8; 32];
                        for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
                            let s = unsafe { std::str::from_utf8_unchecked(chunk) };
                            bytes[i] = u8::from_str_radix(s, 16).unwrap_or(0);
                        }
                        FihHash(bytes)
                    },
                    content,
                    creator: r.creator,
                }
            })
            .collect();

        // Filter intents using record fields directly.
        let intents: Vec<Intent> = all_intents
            .into_iter()
            .filter(|r| {
                if let Some(ref creator) = filter.creator
                    && r.creator != *creator
                {
                    return false;
                }
                if let Some(ref status) = filter.status
                    && simple_status_key(&r.status) != status.as_str()
                {
                    return false;
                }
                if let Some(ref since) = filter.since
                    && let Ok(ts) = since.parse::<u64>()
                {
                    let created_ns = r.created_at * 1_000_000_000;
                    if created_ns < ts {
                        return false;
                    }
                }
                if let Some(ref until) = filter.until
                    && let Ok(ts) = until.parse::<u64>()
                {
                    let created_ns = r.created_at * 1_000_000_000;
                    if created_ns > ts {
                        return false;
                    }
                }
                if let Some(ref ids) = filter.intent_ids {
                    let normalized: Vec<String> = ids
                        .iter()
                        .map(|id| CoordId::from_string(id).to_string())
                        .collect();
                    if !normalized.iter().any(|nid| nid == &r.id) {
                        return false;
                    }
                }
                true
            })
            .map(|r| {
                let description = if r.description_hash.is_empty() {
                    r.id.clone()
                } else {
                    let c = load_content_fast(&r.description_hash, "text/plain");
                    String::from_utf8_lossy(&c.data).to_string()
                };
                Intent {
                    id: CoordId::from_string(&r.id),
                    from_facts: r
                        .from_facts
                        .iter()
                        .map(|s| CoordId::from_string(s))
                        .collect(),
                    description,
                    creator: r.creator,
                    worker: match &r.status {
                        IntentStatus::Claimed { worker, .. }
                        | IntentStatus::Concluded { worker, .. } => Some(worker.clone()),
                        IntentStatus::Submitted => None,
                    },
                    to_fact_id: match &r.status {
                        IntentStatus::Concluded { to_fact, .. } => {
                            Some(CoordId::from_string(to_fact))
                        }
                        _ => None,
                    },
                    last_heartbeat_at: match &r.status {
                        IntentStatus::Claimed {
                            last_heartbeat_at, ..
                        } => Some(*last_heartbeat_at),
                        _ => None,
                    },
                    created_at: Some(r.created_at),
                    is_concluded: matches!(&r.status, IntentStatus::Concluded { .. }),
                    concluded_at: match &r.status {
                        IntentStatus::Concluded { concluded_at, .. } => Some(*concluded_at),
                        _ => None,
                    },
                }
            })
            .collect();

        // Filter hints using record fields directly.
        let hints: Vec<Hint> = all_hints
            .into_iter()
            .filter(|r| {
                if let Some(ref ids) = filter.hint_ids {
                    let normalized: Vec<String> = ids
                        .iter()
                        .map(|id| CoordId::from_string(id).to_string())
                        .collect();
                    if !normalized.iter().any(|nid| nid == &r.id) {
                        return false;
                    }
                }
                true
            })
            .map(|r| Hint {
                id: CoordId::from_string(&r.id),
                content: r.content,
                creator: r.creator,
            })
            .collect();

        // Apply offset/limit.
        let mut state = BoardState {
            facts,
            intents,
            hints,
        };

        let offset = filter.offset.unwrap_or(0);
        if let Some(limit) = filter.limit {
            state.facts = state.facts.into_iter().skip(offset).take(limit).collect();
            state.intents = state.intents.into_iter().skip(offset).take(limit).collect();
            state.hints = state.hints.into_iter().skip(offset).take(limit).collect();
        } else if offset > 0 {
            state.facts = state.facts.into_iter().skip(offset).collect();
            state.intents = state.intents.into_iter().skip(offset).collect();
            state.hints = state.hints.into_iter().skip(offset).collect();
        }

        state
    }
}

// ── AsyncEvictCapable (in-memory eviction) ──────────────────────────────

impl<I: FileIo> crate::AsyncEvictCapable for FihStorage<I> {
    async fn approximate_size(&self) -> usize {
        let facts = self.fact_store.len().await;
        let intents = self.intent_store.len().await;
        let hints = self.hint_store.len().await;
        (facts + intents + hints) * 256
    }

    async fn evict_before(&self, before: &str) -> Result<u64, String> {
        let before_secs: u64 = before.parse().unwrap_or(0);
        let all = self.hint_store.values().await;
        let old_len = all.len();
        let mut kept = Vec::with_capacity(all.len());
        let mut evict_keys = Vec::new();
        for record in all {
            if record.submitted_at >= before_secs {
                kept.push((record.id.clone(), record));
            } else {
                // read_state reads hints from io, so the eviction must
                // also delete the record files; otherwise the evicted
                // hint reappears on the next state read.
                evict_keys.push(WriteOp::Delete { path: record.key() });
            }
        }
        let evicted = (old_len - kept.len()) as u64;
        if !evict_keys.is_empty() {
            self.pending.borrow_mut().extend(evict_keys);
        }
        self.hint_store.replace_from(kept).await;
        self.hint_records
            .borrow_mut()
            .retain(|_, r| r.submitted_at >= before_secs);
        Ok(evicted)
    }

    async fn evict_stale_intents(&self, older_than_secs: u64) -> Result<u64, String> {
        let now = self.clock.now_secs();
        let cutoff = now.saturating_sub(older_than_secs);

        let all = self.intent_store.values().await;
        let old_len = all.len();
        let mut kept = Vec::with_capacity(all.len());
        let mut evict_keys = Vec::new();
        for record in all {
            let stale =
                matches!(record.status, IntentStatus::Submitted) && record.created_at < cutoff;
            if stale {
                // read_state reads intents from io, so the eviction must
                // also delete the record files.
                evict_keys.push(WriteOp::Delete { path: record.key() });
            } else {
                kept.push((record.id.clone(), record));
            }
        }
        let evicted = (old_len - kept.len()) as u64;
        if !evict_keys.is_empty() {
            self.pending.borrow_mut().extend(evict_keys);
        }
        self.intent_store.replace_from(kept).await;
        self.intent_records
            .borrow_mut()
            .retain(|_, r| !(matches!(r.status, IntentStatus::Submitted) && r.created_at < cutoff));
        Ok(evicted)
    }
}

// ── AsyncScanCapable (in-memory scan) ───────────────────────────────────

impl<I: FileIo> crate::AsyncScanCapable for FihStorage<I> {
    async fn scan_partition(&self, partition: &str) -> Result<PartitionData, String> {
        let facts = self.fact_store.values().await;
        let intents = self.intent_store.values().await;
        let hints = self.hint_store.values().await;

        let prefix = format!("partition:{}", partition);
        Ok(PartitionData {
            partition: partition.into(),
            facts: facts
                .into_iter()
                .filter(|f| f.origin == prefix)
                .map(|r| {
                    let content = self.load_content(&r.blob_hash, "application/octet-stream");
                    Fact {
                        id: CoordId::from_string(&r.id),
                        origin: r.origin,
                        content_hash: {
                            let mut h = sha2::Sha256::new();
                            h.update(&content.data);
                            FihHash(h.finalize().into())
                        },
                        content,
                        creator: r.creator,
                    }
                })
                .collect(),
            intents: intents
                .into_iter()
                .filter(|i| i.creator == prefix)
                .map(|r| {
                    let description = if r.description_hash.is_empty() {
                        r.id.clone()
                    } else {
                        let c = self.load_content(&r.description_hash, "text/plain");
                        String::from_utf8_lossy(&c.data).to_string()
                    };
                    Intent {
                        id: CoordId::from_string(&r.id),
                        from_facts: r
                            .from_facts
                            .iter()
                            .map(|s| CoordId::from_string(s))
                            .collect(),
                        description,
                        creator: r.creator,
                        worker: match &r.status {
                            IntentStatus::Claimed { worker, .. }
                            | IntentStatus::Concluded { worker, .. } => Some(worker.clone()),
                            IntentStatus::Submitted => None,
                        },
                        to_fact_id: match &r.status {
                            IntentStatus::Concluded { to_fact, .. } => {
                                Some(CoordId::from_string(to_fact))
                            }
                            _ => None,
                        },
                        last_heartbeat_at: match &r.status {
                            IntentStatus::Claimed {
                                last_heartbeat_at, ..
                            } => Some(*last_heartbeat_at),
                            _ => None,
                        },
                        created_at: Some(r.created_at),
                        is_concluded: matches!(&r.status, IntentStatus::Concluded { .. }),
                        concluded_at: match &r.status {
                            IntentStatus::Concluded { concluded_at, .. } => Some(*concluded_at),
                            _ => None,
                        },
                    }
                })
                .collect(),
            hints: hints
                .into_iter()
                .filter(|h| h.creator == prefix)
                .map(|r| Hint {
                    id: CoordId::from_string(&r.id),
                    content: r.content,
                    creator: r.creator,
                })
                .collect(),
        })
    }
}

// ── AsyncTimeRangeCapable (in-memory time range) ────────────────────────

impl<I: FileIo> crate::AsyncTimeRangeCapable for FihStorage<I> {
    async fn time_range(&self) -> Option<Range<String>> {
        let facts = self.fact_store.values().await;
        if facts.is_empty() {
            return None;
        }
        let min = facts.iter().map(|r| r.submitted_at).min()?;
        let max = facts.iter().map(|r| r.submitted_at).max()?;
        Some(min.to_string()..max.to_string())
    }
}

// ── AsyncFlushCapable (IO: flush_pending via await) ──────────────────────

impl<I: FileIo> crate::AsyncFlushCapable for FihStorage<I> {
    async fn flush_since(&self, cursor: &FlushCursor) -> Result<FlushResult, String> {
        let now_ts = self.clock.now_nanos();

        // Make pending writes durable first.
        self.flush_pending().await?;

        // Collect the durable delta since the cursor: facts and intents on
        // io with a timestamp newer than the cursor. This catches records
        // that were flushed earlier (or by a previous process) but never
        // acked by a cursor, so a reopened store reports the same delta
        // instead of a zero based on the empty pending buffer.
        let facts = self
            .scan_durable_records::<FactRecord>("facts/", |r| {
                r.submitted_at > cursor.last_flushed_at
            })
            .await?;
        let intents = self
            .scan_durable_records::<IntentRecord>("intents/", |r| {
                r.created_at > cursor.last_flushed_at
            })
            .await?;

        let records_flushed = (facts.len() + intents.len()) as u64;

        // Write the delta chain only when there is a delta. The chain file
        // name carries the partition and the flush timestamp, so repeated
        // flushes append one chain per export window. Hints are ephemeral
        // and excluded from the delta, matching the ChainEntry shape.
        if records_flushed > 0 {
            let entry = ChainEntry {
                prev_cursor: cursor.last_flushed_at,
                records_flushed,
                facts,
                intents,
            };
            let bytes = postcard::to_allocvec(&entry)
                .map_err(|e| format!("flush chain encode failed: {e}"))?;
            let chain_key = format!("flush/{}/cursor_{}.chain", cursor.partition, now_ts);
            self.io.write(&chain_key, &bytes).await?;
        }

        Ok(FlushResult {
            records_flushed,
            new_cursor: FlushCursor {
                last_flushed_at: now_ts,
                partition: cursor.partition.clone(),
            },
        })
    }
}
