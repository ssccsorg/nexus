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
    BlackboardError, BoardState, Content, CoordId, Fact, FihHash, Hint, Intent, PartitionData,
    StateFilter,
};
use nex_core::Now;

use crate::core::index::Cell2;
use crate::core::record::{ContentMeta, FactRecord, HintRecord, IntentRecord, IntentStatus};
use crate::io::file_io::{FileIo, WriteOp, default_apply_batch};
use crate::semantic::record::{Query, RecordLoad};
use std::collections::{HashMap, HashSet};

/// Record-layer payload for the unified store (L2 restructure, #176).
///
/// The coordinate tree no longer stores record bodies: it holds id sets
/// at structural paths. `Record` remains the write payload for
/// `place_record`, which fans the fields out to the application layer
/// (record maps), the inverse index (intents), and the tree id set.
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

/// Deterministic u16 fingerprint for an arbitrary string, used for the
/// origin/creator axes of the structural index.
///
/// Advisory only: the fingerprint is a 13.4-bit SHA-256 prefix, so
/// collisions are possible. Record-field predicates compare exact
/// strings; the axes provide ordering, not exact query keys.
fn hash_str(s: &str) -> u16 {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(s.as_bytes());
    let hash = h.finalize();
    u16::from_le_bytes([hash[0], hash[1]]) % 11172
}

/// Build a CoordPath<6> structural path for the unified filter index.
///
/// Axis layout:
///   [0-1]  time (days since epoch, split into two base-11172 axes so the
///          leading-axis lexicographic order equals chronological order
///          for up to u16 x 11172 days; second-level precision stays in
///          the records)
///   [2]    entity
///   [3-4]  origin/creator fingerprints (advisory ordering only)
///   [5]    status
///
/// The path carries no id and no content hash (L2 restructure, #176):
/// the tree value is the set of record ids at the structural path, so
/// the tree is a filter index whose memory is bounded by axis
/// cardinality, not record count. Record bodies live in the application
/// layer (record maps), and the record map's persisted blob hash is the
/// sole defender against same-id collisions.
pub fn structural_path(
    entity: u16,
    origin: &str,
    creator: &str,
    status: u16,
    ts_ns: u64,
) -> tagma_core::CoordPath<6> {
    use tagma_core::{Coord, CoordPath};
    let mk = |v: u16| Coord::new(v % 11172).unwrap();
    let days = ts_ns / 86_400_000_000_000;
    let days_hi = (days / 11172) as u16;
    let days_lo = (days % 11172) as u16;
    // Use hash-based coord for origin/creator (matches make_record_path convention)
    let origin_v = hash_str(origin);
    let creator_v = hash_str(creator);
    let mut coords = [Coord::new(0).unwrap(); 6];
    coords[0] = mk(days_hi);
    coords[1] = mk(days_lo);
    coords[2] = mk(entity);
    coords[3] = mk(origin_v);
    coords[4] = mk(creator_v);
    coords[5] = mk(status);
    CoordPath::new(coords)
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
    // The record maps are the application layer: one id-keyed HashMap per
    // record type, authoritative for reads and writes since the L2
    // restructure (#176). The id-keyed entity store duplication was
    // removed; the maps serve both the internal read paths and the
    // public surface.
    /// Structural filter index: CoordPath<6> (time, entity, origin,
    /// creator, status) to the set of record ids at that path. Memory is
    /// bounded by axis cardinality, not record count (L2 restructure,
    /// #176). The `iter_tree` ascending order contract is preserved.
    store: Cell2<tagma_core::CoordSpaceN<6, Vec<String>>>,
    pub fact_records: Cell2<HashMap<String, FactRecord>>,
    pub intent_records: Cell2<HashMap<String, IntentRecord>>,
    pub hint_records: Cell2<HashMap<String, HintRecord>>,
    /// From-fact to intent ids inverse index (Step 3, #176). Kept in sync
    /// by `place_record` (link) and `vacate_record` (unlink) for every
    /// intent, so `intents_by_fact` is O(fan-out) instead of an O(N)
    /// scan. Keys are the canonical CoordId strings stored in
    /// `IntentRecord::from_facts`; concluded intents stay linked (their
    /// from_facts never change).
    fact_to_intents: Cell2<HashMap<String, Vec<String>>>,
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
            store: Cell2::new(tagma_core::CoordSpaceN::new()),
            fact_records: Cell2::new(HashMap::new()),
            intent_records: Cell2::new(HashMap::new()),
            hint_records: Cell2::new(HashMap::new()),
            fact_to_intents: Cell2::new(HashMap::new()),
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
            store: Cell2::new(tagma_core::CoordSpaceN::new()),
            fact_records: Cell2::new(HashMap::new()),
            intent_records: Cell2::new(HashMap::new()),
            hint_records: Cell2::new(HashMap::new()),
            fact_to_intents: Cell2::new(HashMap::new()),
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

        // Populate the structural filter index and the record maps so id
        // enumeration and spatial queries work after a reopen. The
        // content hash is parsed from the persisted blob hash; a
        // malformed hash falls back to recomputing it from the blob.
        for (_, r) in &facts {
            let content_hash = match Self::hex_blob_hash(&r.blob_hash) {
                Some(h) => h,
                None => {
                    // Legacy or corrupt hash: recompute from the blob.
                    let content = load_blob(&self.io, &r.blob_hash).await;
                    let mut h = sha2::Sha256::new();
                    h.update(&content.data);
                    FihHash(h.finalize().into())
                }
            };
            self.place_record(
                &Self::fact_path(r),
                &r.id,
                Record::Fact {
                    content: Content {
                        mime_type: "application/octet-stream".into(),
                        data: Vec::new(),
                    },
                    content_hash,
                    origin: r.origin.clone(),
                    creator: r.creator.clone(),
                    submitted_at: r.submitted_at,
                },
            );
        }
        for (_, r) in &intents {
            self.place_intent(r);
        }
        for (_, r) in &hints {
            self.place_record(
                &Self::hint_path(r),
                &r.id,
                Record::Hint {
                    content: r.content.clone(),
                    creator: r.creator.clone(),
                    submitted_at: r.submitted_at,
                },
            );
        }

        // The record maps are populated by the place_* calls above; the
        // pre-merge entity stores are gone (L2 restructure, #176).

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

    /// Rebuild semantic stores from fact_store after rebuild_cache.
    pub async fn rebuild_semantic(&self) -> Result<(), String> {
        // Snapshot: take stores atomically, work on them, then put back.
        let mut stores = std::mem::take(&mut *self.semantic_stores.borrow_mut());
        if stores.is_empty() {
            return Ok(());
        }

        let facts: Vec<FactRecord> = self.fact_records.borrow().values().cloned().collect();
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

    /// Normalize an intent id to its canonical CoordId string form. A
    /// non-canonical id cannot reference any stored intent.
    fn normalize_intent_id(&self, intent_id: &str) -> String {
        CoordId::resolve(intent_id).to_string()
    }

    /// Query intents that reference a given fact.
    ///
    /// The from-fact to intent inverse index is kept in sync by
    /// `place_record` and `vacate_record` (Step 3, #176), so this is an
    /// O(fan-out) lookup instead of an O(N) scan. The fact_id is resolved
    /// via CoordId::resolve to match the canonical CoordId format stored
    /// in IntentRecord.from_facts. Concluded intents remain referenced
    /// (their from_facts never change).
    pub fn intents_by_fact(&self, fact_id: &str) -> Vec<String> {
        let normalized = crate::CoordId::resolve(fact_id).to_string();
        self.fact_to_intents
            .borrow()
            .get(&normalized)
            .cloned()
            .unwrap_or_default()
    }

    /// Direct record placement (for special cases like nex-calc) and the
    /// single chokepoint for the record layer.
    ///
    /// Maintains three structures in one call:
    ///   - the application-layer record map (`fact_records`,
    ///     `intent_records`, `hint_records`), so reads (id enumeration,
    ///     lookups, filtered traversal) see every writer including direct
    ///     ones. For facts the persisted blob hash is the sole defender
    ///     against same-id collisions;
    ///   - the from-fact to intent inverse index (`fact_to_intents`,
    ///     intents only), the O(1) reverse lookup;
    ///   - the structural filter index (id set at the structural path).
    ///
    /// Blob enqueue is skipped (the caller owns blob IO). The conflict
    /// guard lives in `submit_fact` (the mission scope): a direct writer
    /// that places the same id with a different `content_hash` silently
    /// overwrites here, and the next `submit_fact` against that id is
    /// rejected. Direct writers must therefore be id-stable
    /// (deterministic content per id), which nex-calc is
    /// (`make_number_fact_id` and the content hash both derive from the
    /// value). In debug builds the invariant is asserted.
    pub fn place_record(&self, path: &tagma_core::CoordPath<6>, id: &str, record: Record) {
        match &record {
            Record::Fact {
                content_hash,
                origin,
                creator,
                submitted_at,
                ..
            } => {
                if let Some(existing) = self.fact_records.borrow().get(id)
                    && let Some(existing_hash) = Self::hex_blob_hash(&existing.blob_hash)
                {
                    debug_assert!(
                        existing_hash == *content_hash,
                        "place_record: fact id {id} is being overwritten with a different \
                         content_hash; direct writers must be id-stable or route through \
                         submit_fact"
                    );
                }
                self.fact_records.borrow_mut().insert(
                    id.to_string(),
                    FactRecord {
                        id: id.to_string(),
                        blob_hash: content_hash.to_string(),
                        origin: origin.clone(),
                        creator: creator.clone(),
                        submitted_at: *submitted_at,
                    },
                );
            }
            Record::Intent {
                from_facts,
                description_hash,
                creator,
                status,
                created_at,
            } => {
                self.intent_records.borrow_mut().insert(
                    id.to_string(),
                    IntentRecord {
                        id: id.to_string(),
                        from_facts: from_facts.clone(),
                        description_hash: description_hash.clone(),
                        creator: creator.clone(),
                        status: status.clone(),
                        created_at: *created_at,
                    },
                );
                // Link the inverse index: this intent references each
                // from-fact (Step 3, #176). Status moves relink the same
                // from_facts, so the net entry is unchanged.
                let mut index = self.fact_to_intents.borrow_mut();
                for fact in from_facts {
                    let entry = index.entry(fact.clone()).or_default();
                    if !entry.iter().any(|x| x == id) {
                        entry.push(id.to_string());
                    }
                }
            }
            Record::Hint {
                content,
                creator,
                submitted_at,
            } => {
                self.hint_records.borrow_mut().insert(
                    id.to_string(),
                    HintRecord {
                        id: id.to_string(),
                        content: content.clone(),
                        creator: creator.clone(),
                        submitted_at: *submitted_at,
                        ttl_secs: None,
                    },
                );
            }
        }
        // Add the id to the structural path's id set.
        let mut store = self.store.borrow_mut();
        match store.at_path_mut(path) {
            Some(ids) => {
                if !ids.iter().any(|x| x == id) {
                    ids.push(id.to_string());
                }
            }
            None => {
                store.place_path(path, vec![id.to_string()]);
            }
        }
    }

    /// Direct record removal (for special cases like nex-calc).
    ///
    /// Removes `id` from the authoritative record map (the entity axis
    /// [2] selects fact/intent/hint), from the inverse index (intents),
    /// and from the id set at the structural path. Facts are
    /// append-only, so vacate is used only for intents (status moves)
    /// and hints; the fact branches exist for completeness and also
    /// remove the record-map entry.
    pub fn vacate_record(&self, path: &tagma_core::CoordPath<6>, id: &str) {
        match path.coords()[2].index() {
            0 => {
                self.fact_records.borrow_mut().remove(id);
            }
            1 => {
                // Unlink the inverse index before removing the record:
                // the record map still holds the intent, so its
                // from_facts are available.
                let from_facts: Vec<String> = self
                    .intent_records
                    .borrow()
                    .get(id)
                    .map(|r| r.from_facts.clone())
                    .unwrap_or_default();
                let mut index = self.fact_to_intents.borrow_mut();
                for fact in from_facts {
                    if let Some(entry) = index.get_mut(&fact) {
                        entry.retain(|x| x != id);
                    }
                }
                self.intent_records.borrow_mut().remove(id);
            }
            2 => {
                self.hint_records.borrow_mut().remove(id);
            }
            _ => {}
        }
        // Remove the id from the structural path's id set.
        let mut store = self.store.borrow_mut();
        if let Some(ids) = store.at_path_mut(path) {
            ids.retain(|x| x != id);
        }
    }

    /// CoordPath<6> for a fact record: entity=0, status=0, time axis from
    /// the nanosecond `submitted_at`.
    fn fact_path(record: &FactRecord) -> tagma_core::CoordPath<6> {
        structural_path(
            0u16,
            &record.origin,
            &record.creator,
            0u16,
            record.submitted_at,
        )
    }

    /// CoordPath<6> for an intent record under the given status: entity=1,
    /// status axis 0/1/2 (Submitted/Claimed/Concluded). `created_at` is
    /// stored in seconds; the path time axis is nanoseconds.
    fn intent_path_with(record: &IntentRecord, status: &IntentStatus) -> tagma_core::CoordPath<6> {
        let status_coord = match status {
            IntentStatus::Submitted => 0u16,
            IntentStatus::Claimed { .. } => 1u16,
            IntentStatus::Concluded { .. } => 2u16,
        };
        structural_path(
            1u16,
            "",
            &record.creator,
            status_coord,
            record.created_at * 1_000_000_000,
        )
    }

    /// CoordPath<6> for an intent record under its current status.
    fn intent_path(record: &IntentRecord) -> tagma_core::CoordPath<6> {
        Self::intent_path_with(record, &record.status)
    }

    /// CoordPath<6> for a hint record: entity=2. `submitted_at` is stored
    /// in seconds; the path time axis is nanoseconds.
    fn hint_path(record: &HintRecord) -> tagma_core::CoordPath<6> {
        structural_path(
            2u16,
            "",
            &record.creator,
            0u16,
            record.submitted_at * 1_000_000_000,
        )
    }

    /// Place an intent in the record layer under its current status.
    fn place_intent(&self, record: &IntentRecord) {
        self.place_record(
            &Self::intent_path(record),
            &record.id,
            Record::Intent {
                from_facts: record.from_facts.clone(),
                description_hash: record.description_hash.clone(),
                creator: record.creator.clone(),
                status: record.status.clone(),
                created_at: record.created_at,
            },
        );
    }

    /// Resolve a semantic index back to its ID string.
    pub fn resolve_semantic_idx(&self, idx: u32) -> String {
        let records: Vec<FactRecord> = self.fact_records.borrow().values().cloned().collect();
        records
            .get(idx as usize)
            .map(|r| r.id.clone())
            .unwrap_or_default()
    }

    /// Check if a fact with the given ID exists (fast-path: fact_records HashMap).
    pub fn fact_exists(&self, id: &str) -> bool {
        self.fact_records.borrow().contains_key(id)
    }

    /// Content hash of the fact currently occupying `id`, if any.
    ///
    /// The id-keyed record map is the single chokepoint for the record
    /// layer, so this is an O(1) lookup for every writer: `submit_fact`,
    /// `rebuild_cache` (which re-places loaded records), and direct
    /// writers (nex-calc). The hash is parsed from the persisted blob
    /// hash, so the check also fires after `rebuild_cache`.
    fn existing_fact_content_hash(&self, id: &str) -> Option<FihHash> {
        self.fact_records
            .borrow()
            .get(id)
            .and_then(|r| Self::hex_blob_hash(&r.blob_hash))
    }

    /// Parse a 64-char lowercase hex blob hash back into `FihHash`.
    /// `FactRecord::blob_hash` is written by `FihHash::to_string`, so the
    /// format is fixed; a malformed length or hex digit is corruption.
    fn hex_blob_hash(hex: &str) -> Option<FihHash> {
        let hex = hex.as_bytes();
        if hex.len() != 64 {
            return None;
        }
        let mut bytes = [0u8; 32];
        for i in 0..32 {
            let hi = (hex[i * 2] as char).to_digit(16)?;
            let lo = (hex[i * 2 + 1] as char).to_digit(16)?;
            bytes[i] = ((hi << 4) | lo) as u8;
        }
        Some(FihHash(bytes))
    }

    /// Check if an intent with the given ID exists (fast-path: intent_records HashMap).
    pub fn intent_exists(&self, id: &str) -> bool {
        self.intent_records.borrow().contains_key(id)
    }

    /// Check if a hint with the given ID exists (fast-path: hint_records HashMap).
    pub fn hint_exists(&self, id: &str) -> bool {
        self.hint_records.borrow().contains_key(id)
    }

    /// Returns all fact IDs (record-map keys; the record maps are the
    /// authoritative record layer since the L2 restructure, #176).
    pub fn all_fact_ids(&self) -> Vec<String> {
        self.fact_records.borrow().keys().cloned().collect()
    }

    /// Returns all intent IDs (record-map keys).
    pub fn all_intent_ids(&self) -> Vec<String> {
        self.intent_records.borrow().keys().cloned().collect()
    }

    /// Returns all hint IDs (record-map keys).
    pub fn all_hint_ids(&self) -> Vec<String> {
        self.hint_records.borrow().keys().cloned().collect()
    }

    /// Get a fact by its ID (record-map lookup).
    ///
    /// Content is materialized from pending writes first, then from IO by
    /// the persisted blob hash, so records placed by direct writers
    /// (nex-calc writes blobs to IO directly) are readable.
    pub async fn get_fact_by_id(&self, id: &str) -> Option<(Content, FihHash, String, String)> {
        let r = {
            let recs = self.fact_records.borrow();
            recs.get(id)?.clone()
        };
        let content_hash = Self::hex_blob_hash(&r.blob_hash).unwrap_or(FihHash([0u8; 32]));
        let content = self
            .load_content_any(&r.blob_hash, "application/octet-stream")
            .await;
        Some((content, content_hash, r.origin.clone(), r.creator.clone()))
    }

    /// Get an intent by its ID (record-map lookup).
    pub fn get_intent_by_id(
        &self,
        id: &str,
    ) -> Option<(Vec<String>, String, String, IntentStatus, u64)> {
        let recs = self.intent_records.borrow();
        let r = recs.get(id)?;
        Some((
            r.from_facts.clone(),
            r.description_hash.clone(),
            r.creator.clone(),
            r.status.clone(),
            r.created_at,
        ))
    }

    /// Get a hint by its ID (record-map lookup).
    pub fn get_hint_by_id(&self, id: &str) -> Option<(String, String, u64)> {
        let recs = self.hint_records.borrow();
        let r = recs.get(id)?;
        Some((r.content.clone(), r.creator.clone(), r.submitted_at))
    }

    /// Load blob content from pending writes. No IO fallback — the sync
    /// path only has access to in-memory caches; after `flush_pending` +
    /// `rebuild_cache` the content lives in IO and `load_content_any`
    /// crosses the async boundary instead.
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

        Content {
            mime_type: default_mime.to_string(),
            data: Vec::new(),
        }
    }

    /// Load blob content from pending writes, falling back to IO by hash.
    /// Async because the IO fallback awaits; callers must not hold a
    /// record-map or tree borrow across the call.
    async fn load_content_any(&self, blob_hash: &str, default_mime: &str) -> Content {
        let pending_content = self.load_content(blob_hash, default_mime);
        if !pending_content.data.is_empty() {
            return pending_content;
        }
        load_blob(&self.io, blob_hash).await
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
                        id: CoordId::resolve(&r.id),
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
                        id: CoordId::resolve(&r.id),
                        from_facts: r.from_facts.iter().map(|s| CoordId::resolve(s)).collect(),
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
                                Some(CoordId::resolve(to_fact))
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
                        id: CoordId::resolve(&r.id),
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
        let id = fact.id.to_string();
        // The id is the record-layer key. A second fact at the same id is
        // legitimate only when it is the identical content (an idempotent
        // retry); a different content_hash means the id is not a safe
        // content address and the earlier record must not be overwritten.
        if let Some(existing_hash) = self.existing_fact_content_hash(&id) {
            if existing_hash != fact.content_hash {
                return Err(BlackboardError::Conflict(format!(
                    "fact id {id} already exists with a different content_hash"
                )));
            }
            return Ok(fact.id);
        }

        // content_hash is SHA-256 already computed by Fact::new — use directly.
        let blob_hash = fact.content_hash.to_string();
        let pending_len = self.pending.borrow().len();

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

        // Update in-memory cache immediately for subsequent reads. The
        // return value is the atomic detector at the first id-keyed
        // commit: it catches a record the pre-check could not see (a
        // direct record-map write that bypassed the check) and a task
        // that raced past the pre-check if the insert ever yields.
        let prev = self
            .fact_records
            .borrow_mut()
            .insert(record.id.clone(), record.clone());
        if let Some(prev_record) = prev {
            // Occupied at the commit point. Restore the earlier record
            // (keep its submitted_at) and drop the blob ops enqueued by
            // this submit.
            let same = Self::hex_blob_hash(&prev_record.blob_hash)
                .map(|h| h == fact.content_hash)
                .unwrap_or(false);
            self.fact_records
                .borrow_mut()
                .insert(record.id.clone(), prev_record);
            self.pending.borrow_mut().truncate(pending_len);
            if !same {
                return Err(BlackboardError::Conflict(format!(
                    "fact id {id} already exists with a different content_hash"
                )));
            }
            return Ok(fact.id);
        }
        // Record layer: structural index (time/entity/origin/creator/status)
        // plus the record map and id set, all maintained by
        // `place_record`. The entity store insert above is the atomic
        // occupancy detector; this block runs only on the fresh-insert path.
        self.place_record(
            &Self::fact_path(&record),
            &record.id,
            Record::Fact {
                content: fact.content.clone(),
                content_hash: fact.content_hash,
                origin: fact.origin.clone(),
                creator: fact.creator.clone(),
                submitted_at: record.submitted_at,
            },
        );
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
        self.place_record(
            &Self::hint_path(&record),
            &record.id,
            Record::Hint {
                content: record.content.clone(),
                creator: record.creator.clone(),
                submitted_at: record.submitted_at,
            },
        );
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
            if !self.fact_records.borrow().contains_key(&fid_str) {
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

        self.place_intent(&record);
        self.pending.borrow_mut().push(op);
        Ok(intent.id)
    }

    async fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.flush_pending()
            .await
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        let normalized = self.normalize_intent_id(intent_id);
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
        let old_status = record.status.clone();
        let new_status = record.status.try_claim(agent, now).map_err(|e| {
            if e.starts_with("already claimed") {
                BlackboardError::Conflict(e)
            } else {
                BlackboardError::Internal(e)
            }
        })?;
        let old_path = Self::intent_path_with(&record, &old_status);
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
        // Status move in the record layer: only after the io commit
        // succeeds, so a failed flush leaves the store consistent with io.
        self.vacate_record(&old_path, &normalized);
        self.place_intent(&record);
        Ok(())
    }

    async fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.flush_pending()
            .await
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        let normalized = self.normalize_intent_id(intent_id);
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
        let old_status = record.status.clone();
        let new_status = record.status.try_heartbeat(agent, now).map_err(|e| {
            if e.contains("not") {
                BlackboardError::Conflict(e)
            } else {
                BlackboardError::Internal(e)
            }
        })?;
        let old_path = Self::intent_path_with(&record, &old_status);
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
        // Status move in the record layer: only after the io commit
        // succeeds, so a failed flush leaves the store consistent with io.
        self.vacate_record(&old_path, &normalized);
        self.place_intent(&record);
        Ok(())
    }

    async fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.flush_pending()
            .await
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        let normalized = self.normalize_intent_id(intent_id);
        let key = format!("intents/i_{}.intent", normalized);
        let bytes = self
            .io
            .read(&key)
            .await
            .map_err(BlackboardError::Internal)?
            .ok_or_else(|| BlackboardError::NotFound(format!("Intent {intent_id} not found")))?;
        let mut record = postcard::from_bytes::<IntentRecord>(&bytes)
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;

        let old_status = record.status.clone();
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

        // Status move in the record layer: vacate the claimed path, place
        // the submitted path.
        let old_path = Self::intent_path_with(&record, &old_status);

        let bytes =
            postcard::to_allocvec(&record).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.pending.borrow_mut().push(WriteOp::Write {
            path: key,
            data: bytes,
        });
        self.flush_pending()
            .await
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        // Status move in the record layer: only after the io commit
        // succeeds, so a failed flush leaves the store consistent with io.
        self.vacate_record(&old_path, &normalized);
        self.place_intent(&record);
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
        let normalized = self.normalize_intent_id(intent_id);
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

        let conclusion_id = CoordId::from_label(&format!("f_concl_{}", intent_id)).to_string();
        let content_data = result.as_bytes().to_vec();
        let content_hash = {
            let mut h = sha2::Sha256::new();
            h.update(&content_data);
            FihHash(h.finalize().into())
        };
        let new_fact = Fact {
            id: CoordId::resolve(&conclusion_id),
            content_hash,
            origin: format!("conclusion:{}", intent_id),
            content: Content {
                mime_type: "text/plain".into(),
                data: result.as_bytes().to_vec(),
            },
            creator: worker.clone(),
        };

        let now_ns = self.clock.now_nanos();
        let old_status = record.status.clone();
        record.status = record
            .status
            .try_conclude(&conclusion_id, now_ns)
            .map_err(BlackboardError::Internal)?;
        let old_path = Self::intent_path_with(&record, &old_status);

        // Write conclusion fact and updated intent via pending buffer.
        // The conclusion fact carries the conclude time (now_ns) so it
        // lands on the real day in the record layer and time_range
        // reflects it. It is blob-backed like a submitted fact so a
        // reopen materializes its content and hash consistently.
        let blob_hash = new_fact.content_hash.to_string();
        let meta = ContentMeta {
            mime_type: new_fact.content.mime_type.clone(),
            size: content_data.len() as u64,
        };
        let meta_bytes =
            postcard::to_allocvec(&meta).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.pending.borrow_mut().push(WriteOp::Write {
            path: format!("blob/{blob_hash}.bin"),
            data: content_data.clone(),
        });
        self.pending.borrow_mut().push(WriteOp::Write {
            path: format!("blob/{blob_hash}.bin.meta"),
            data: meta_bytes,
        });
        let fact_rec = FactRecord::from_model(&new_fact, blob_hash, now_ns);
        let fact_bytes = postcard::to_allocvec(&fact_rec)
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.pending.borrow_mut().push(WriteOp::Write {
            path: fact_rec.key(),
            data: fact_bytes,
        });
        self.fact_records
            .borrow_mut()
            .insert(fact_rec.id.clone(), fact_rec.clone());

        let intent_bytes =
            postcard::to_allocvec(&record).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.pending.borrow_mut().push(WriteOp::Write {
            path: key,
            data: intent_bytes,
        });
        self.flush_pending()
            .await
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        // Record-layer moves happen only after the io commit succeeds, so
        // a failed flush leaves the store consistent with io: vacate the
        // old status path, place the concluded intent and the conclusion
        // fact at the conclude time.
        self.vacate_record(&old_path, &normalized);
        self.place_intent(&record);
        self.place_record(
            &Self::fact_path(&fact_rec),
            &fact_rec.id,
            Record::Fact {
                content: new_fact.content.clone(),
                content_hash: new_fact.content_hash,
                origin: new_fact.origin.clone(),
                creator: new_fact.creator.clone(),
                submitted_at: fact_rec.submitted_at,
            },
        );

        Ok(new_fact)
    }
}

// ── AsyncFilterCapable (in-memory filtering) ────────────────────────────

impl<I: FileIo> crate::AsyncFilterCapable for FihStorage<I> {
    async fn read_state_filtered(&self, filter: &StateFilter) -> BoardState {
        // Build blob lookup map once from pending writes (avoid O(N×P)
        // scan). Data and meta writes merge per blob hash: the data
        // entry carries the payload, the meta entry the mime type.
        let mut blob_map: HashMap<String, (String, Vec<u8>)> = HashMap::new();
        for op in self.pending.borrow().iter() {
            match op {
                WriteOp::Write { path, data }
                    if path.ends_with(".bin") && !path.ends_with(".bin.meta") =>
                {
                    if let Some(blob_hash) = path
                        .strip_prefix("blob/")
                        .and_then(|p| p.strip_suffix(".bin"))
                    {
                        blob_map
                            .entry(blob_hash.to_string())
                            .or_insert_with(|| (String::new(), Vec::new()))
                            .1 = data.clone();
                    }
                }
                WriteOp::Write { path, data } if path.ends_with(".bin.meta") => {
                    if let Some(blob_hash) = path
                        .strip_prefix("blob/")
                        .and_then(|p| p.strip_suffix(".bin.meta"))
                        && let Ok(meta) = postcard::from_bytes::<ContentMeta>(data)
                    {
                        blob_map
                            .entry(blob_hash.to_string())
                            .or_insert_with(|| (String::new(), Vec::new()))
                            .0 = meta.mime_type;
                    }
                }
                _ => {}
            }
        }

        // Materialize content from pending blobs (no io fallback).
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

        // Predicates apply to record fields. The traversal is a full scan
        // over the application-layer record maps, the authoritative record
        // layer since the L2 restructure (#176). The structural index is
        // maintained for spatial prefix queries but adds no pruning here:
        // the hashed origin/creator axes are advisory and would introduce
        // hash-collision false positives and negatives.
        let since: Option<u64> = filter.since.as_ref().and_then(|s| s.parse().ok());
        let until: Option<u64> = filter.until.as_ref().and_then(|s| s.parse().ok());

        let mut facts = Vec::new();
        let mut intents = Vec::new();
        let mut hints = Vec::new();
        // Blob hashes to materialize after the record-map borrows are
        // released, so the async io fallback never awaits while holding a
        // borrow: (index, blob hash).
        let mut fact_blob_jobs: Vec<(usize, String)> = Vec::new();
        let mut desc_jobs: Vec<(usize, String)> = Vec::new();

        {
            let fact_recs = self.fact_records.borrow();
            for (id, r) in fact_recs.iter() {
                if let Some(ref want) = filter.origin
                    && &r.origin != want
                {
                    continue;
                }
                if let Some(ref want) = filter.creator
                    && &r.creator != want
                {
                    continue;
                }
                if let Some(ts) = since
                    && r.submitted_at < ts
                {
                    continue;
                }
                if let Some(ts) = until
                    && r.submitted_at > ts
                {
                    continue;
                }
                if let Some(ids) = filter.fact_ids.as_ref() {
                    let canonical = CoordId::resolve(id).to_string();
                    if !ids
                        .iter()
                        .any(|x| CoordId::resolve(x).to_string() == canonical)
                    {
                        continue;
                    }
                }
                let content_hash = Self::hex_blob_hash(&r.blob_hash).unwrap_or(FihHash([0u8; 32]));
                fact_blob_jobs.push((facts.len(), r.blob_hash.clone()));
                facts.push(Fact {
                    id: CoordId::resolve(id),
                    origin: r.origin.clone(),
                    content_hash,
                    content: Content {
                        mime_type: "application/octet-stream".into(),
                        data: Vec::new(),
                    },
                    creator: r.creator.clone(),
                });
            }
        }
        {
            let intent_recs = self.intent_records.borrow();
            for (id, r) in intent_recs.iter() {
                if let Some(ref want) = filter.creator
                    && &r.creator != want
                {
                    continue;
                }
                let created_ns = r.created_at * 1_000_000_000;
                if let Some(ts) = since
                    && created_ns < ts
                {
                    continue;
                }
                if let Some(ts) = until
                    && created_ns > ts
                {
                    continue;
                }
                if let Some(st) = filter.status.as_ref()
                    && simple_status_key(&r.status) != st.as_str()
                {
                    continue;
                }
                if let Some(ids) = filter.intent_ids.as_ref() {
                    let canonical = CoordId::resolve(id).to_string();
                    if !ids
                        .iter()
                        .any(|x| CoordId::resolve(x).to_string() == canonical)
                    {
                        continue;
                    }
                }
                let description = if r.description_hash.is_empty() {
                    id.clone()
                } else {
                    desc_jobs.push((intents.len(), r.description_hash.clone()));
                    String::new()
                };
                intents.push(Intent {
                    id: CoordId::resolve(id),
                    from_facts: r
                        .from_facts
                        .iter()
                        .map(|s| CoordId::from_string(s).expect("stored from_facts are canonical"))
                        .collect(),
                    description,
                    creator: r.creator.clone(),
                    worker: match &r.status {
                        IntentStatus::Claimed { worker, .. }
                        | IntentStatus::Concluded { worker, .. } => Some(worker.clone()),
                        IntentStatus::Submitted => None,
                    },
                    to_fact_id: match &r.status {
                        IntentStatus::Concluded { to_fact, .. } => Some(CoordId::resolve(to_fact)),
                        _ => None,
                    },
                    last_heartbeat_at: match &r.status {
                        IntentStatus::Claimed {
                            last_heartbeat_at, ..
                        } => Some(*last_heartbeat_at),
                        _ => None,
                    },
                    created_at: Some(r.created_at),
                    is_concluded: matches!(r.status, IntentStatus::Concluded { .. }),
                    concluded_at: match &r.status {
                        IntentStatus::Concluded { concluded_at, .. } => Some(*concluded_at),
                        _ => None,
                    },
                });
            }
        }
        {
            let hint_recs = self.hint_records.borrow();
            for (id, r) in hint_recs.iter() {
                if let Some(ref want) = filter.creator
                    && &r.creator != want
                {
                    continue;
                }
                if let Some(ids) = filter.hint_ids.as_ref() {
                    let canonical = CoordId::resolve(id).to_string();
                    if !ids
                        .iter()
                        .any(|x| CoordId::resolve(x).to_string() == canonical)
                    {
                        continue;
                    }
                }
                hints.push(Hint {
                    id: CoordId::resolve(id),
                    content: r.content.clone(),
                    creator: r.creator.clone(),
                });
            }
        }

        // Materialize fact content and intent descriptions now that the
        // record-map borrows are released. Distinct blob hashes are
        // loaded once each from IO (content dedup makes many records
        // share a blob), then every job resolves from the shared map.
        let mut io_blobs: HashMap<String, Content> = HashMap::new();
        let mut seen: HashSet<String> = HashSet::new();
        for (_, hash) in fact_blob_jobs.iter() {
            let c = load_content_fast(hash, "application/octet-stream");
            if c.data.is_empty() && seen.insert(hash.clone()) {
                io_blobs.insert(hash.clone(), load_blob(&self.io, hash).await);
            }
        }
        for (_, hash) in desc_jobs.iter() {
            let c = load_content_fast(hash, "text/plain");
            if c.data.is_empty() && seen.insert(hash.clone()) {
                io_blobs.insert(hash.clone(), load_blob(&self.io, hash).await);
            }
        }
        for (idx, hash) in fact_blob_jobs {
            let c = load_content_fast(&hash, "application/octet-stream");
            facts[idx].content = if c.data.is_empty() {
                io_blobs.get(&hash).cloned().unwrap_or_else(|| Content {
                    mime_type: "application/octet-stream".into(),
                    data: Vec::new(),
                })
            } else {
                c
            };
        }
        for (idx, hash) in desc_jobs {
            let c = load_content_fast(&hash, "text/plain");
            let text = if c.data.is_empty() {
                String::from_utf8_lossy(
                    io_blobs
                        .get(&hash)
                        .map(|c| c.data.as_slice())
                        .unwrap_or(b""),
                )
                .to_string()
            } else {
                String::from_utf8_lossy(&c.data).to_string()
            };
            intents[idx].description = text;
        }

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
        let facts = self.fact_records.borrow().len();
        let intents = self.intent_records.borrow().len();
        let hints = self.hint_records.borrow().len();
        (facts + intents + hints) * 256
    }

    async fn evict_before(&self, before: &str) -> Result<u64, String> {
        let before_secs: u64 = before.parse().unwrap_or(0);
        let all: Vec<HintRecord> = self.hint_records.borrow().values().cloned().collect();
        let old_len = all.len();
        let mut kept = 0usize;
        let mut evict_keys = Vec::new();
        for record in all {
            if record.submitted_at >= before_secs {
                kept += 1;
            } else {
                // read_state reads hints from io, so the eviction must
                // also delete the record files; otherwise the evicted
                // hint reappears on the next state read.
                evict_keys.push(WriteOp::Delete { path: record.key() });
                self.vacate_record(&Self::hint_path(&record), &record.id);
            }
        }
        let evicted = (old_len - kept) as u64;
        if !evict_keys.is_empty() {
            self.pending.borrow_mut().extend(evict_keys);
        }
        self.hint_records
            .borrow_mut()
            .retain(|_, r| r.submitted_at >= before_secs);
        Ok(evicted)
    }

    async fn evict_stale_intents(&self, older_than_secs: u64) -> Result<u64, String> {
        let now = self.clock.now_secs();
        let cutoff = now.saturating_sub(older_than_secs);

        let all: Vec<IntentRecord> = self.intent_records.borrow().values().cloned().collect();
        let old_len = all.len();
        let mut kept = 0usize;
        let mut evict_keys = Vec::new();
        for record in all {
            let stale =
                matches!(record.status, IntentStatus::Submitted) && record.created_at < cutoff;
            if stale {
                // read_state reads intents from io, so the eviction must
                // also delete the record files.
                evict_keys.push(WriteOp::Delete { path: record.key() });
                self.vacate_record(&Self::intent_path(&record), &record.id);
            } else {
                kept += 1;
            }
        }
        let evicted = (old_len - kept) as u64;
        if !evict_keys.is_empty() {
            self.pending.borrow_mut().extend(evict_keys);
        }
        self.intent_records
            .borrow_mut()
            .retain(|_, r| !(matches!(r.status, IntentStatus::Submitted) && r.created_at < cutoff));
        Ok(evicted)
    }
}

// ── AsyncScanCapable (in-memory scan) ───────────────────────────────────

impl<I: FileIo> crate::AsyncScanCapable for FihStorage<I> {
    async fn scan_partition(&self, partition: &str) -> Result<PartitionData, String> {
        let prefix = format!("partition:{}", partition);

        // Partition scan over the application-layer record maps (the
        // authoritative record layer since the L2 restructure, #176).
        // Facts match on the origin field, intents and hints on the
        // creator field (the existing per-type convention). The
        // comparison is exact on the record strings.
        let mut facts = Vec::new();
        let mut intents = Vec::new();
        let mut hints = Vec::new();
        // Blob hashes to materialize after the record-map borrows are
        // released, so the async io fallback never awaits while holding a
        // borrow: (index, blob hash).
        let mut fact_blob_jobs: Vec<(usize, String)> = Vec::new();
        let mut desc_jobs: Vec<(usize, String)> = Vec::new();

        {
            let fact_recs = self.fact_records.borrow();
            for (id, r) in fact_recs.iter() {
                if r.origin != prefix {
                    continue;
                }
                let content_hash = Self::hex_blob_hash(&r.blob_hash).unwrap_or(FihHash([0u8; 32]));
                fact_blob_jobs.push((facts.len(), r.blob_hash.clone()));
                facts.push(Fact {
                    id: CoordId::resolve(id),
                    origin: r.origin.clone(),
                    content_hash,
                    content: Content {
                        mime_type: "application/octet-stream".into(),
                        data: Vec::new(),
                    },
                    creator: r.creator.clone(),
                });
            }
        }
        {
            let intent_recs = self.intent_records.borrow();
            for (id, r) in intent_recs.iter() {
                if r.creator != prefix {
                    continue;
                }
                let description = if r.description_hash.is_empty() {
                    id.clone()
                } else {
                    desc_jobs.push((intents.len(), r.description_hash.clone()));
                    String::new()
                };
                intents.push(Intent {
                    id: CoordId::resolve(id),
                    from_facts: r
                        .from_facts
                        .iter()
                        .map(|s| CoordId::from_string(s).expect("stored from_facts are canonical"))
                        .collect(),
                    description,
                    creator: r.creator.clone(),
                    worker: match &r.status {
                        IntentStatus::Claimed { worker, .. }
                        | IntentStatus::Concluded { worker, .. } => Some(worker.clone()),
                        IntentStatus::Submitted => None,
                    },
                    to_fact_id: match &r.status {
                        IntentStatus::Concluded { to_fact, .. } => Some(CoordId::resolve(to_fact)),
                        _ => None,
                    },
                    last_heartbeat_at: match &r.status {
                        IntentStatus::Claimed {
                            last_heartbeat_at, ..
                        } => Some(*last_heartbeat_at),
                        _ => None,
                    },
                    created_at: Some(r.created_at),
                    is_concluded: matches!(r.status, IntentStatus::Concluded { .. }),
                    concluded_at: match &r.status {
                        IntentStatus::Concluded { concluded_at, .. } => Some(*concluded_at),
                        _ => None,
                    },
                });
            }
        }
        {
            let hint_recs = self.hint_records.borrow();
            for (id, r) in hint_recs.iter() {
                if r.creator != prefix {
                    continue;
                }
                hints.push(Hint {
                    id: CoordId::resolve(id),
                    content: r.content.clone(),
                    creator: r.creator.clone(),
                });
            }
        }

        // Materialize fact content and intent descriptions now that the
        // record-map borrows are released. Distinct blob hashes are
        // loaded once each from IO (content dedup makes many records
        // share a blob), then every job resolves from the shared map.
        let mut io_blobs: HashMap<String, Content> = HashMap::new();
        let mut seen: HashSet<String> = HashSet::new();
        for (_, hash) in fact_blob_jobs.iter() {
            let c = self.load_content(hash, "application/octet-stream");
            if c.data.is_empty() && seen.insert(hash.clone()) {
                io_blobs.insert(hash.clone(), load_blob(&self.io, hash).await);
            }
        }
        for (_, hash) in desc_jobs.iter() {
            let c = self.load_content(hash, "text/plain");
            if c.data.is_empty() && seen.insert(hash.clone()) {
                io_blobs.insert(hash.clone(), load_blob(&self.io, hash).await);
            }
        }
        for (idx, hash) in fact_blob_jobs {
            let c = self.load_content(&hash, "application/octet-stream");
            facts[idx].content = if c.data.is_empty() {
                io_blobs.get(&hash).cloned().unwrap_or_else(|| Content {
                    mime_type: "application/octet-stream".into(),
                    data: Vec::new(),
                })
            } else {
                c
            };
        }
        for (idx, hash) in desc_jobs {
            let c = self.load_content(&hash, "text/plain");
            let text = if c.data.is_empty() {
                String::from_utf8_lossy(
                    io_blobs
                        .get(&hash)
                        .map(|c| c.data.as_slice())
                        .unwrap_or(b""),
                )
                .to_string()
            } else {
                String::from_utf8_lossy(&c.data).to_string()
            };
            intents[idx].description = text;
        }

        Ok(PartitionData {
            partition: partition.into(),
            facts,
            intents,
            hints,
        })
    }
}

// ── AsyncTimeRangeCapable (in-memory time range) ────────────────────────

impl<I: FileIo> crate::AsyncTimeRangeCapable for FihStorage<I> {
    async fn time_range(&self) -> Option<Range<String>> {
        // Exact min/max over the Fact records in the application-layer
        // record map. The structural index is coordinate-ordered, but
        // within a boundary day the order is set by the other axes, not
        // the timestamp, so the first/last Fact in tree order cannot
        // bound the range exactly.
        let fact_recs = self.fact_records.borrow();
        let mut min = u64::MAX;
        let mut max = u64::MIN;
        let mut found = false;
        for r in fact_recs.values() {
            found = true;
            min = min.min(r.submitted_at);
            max = max.max(r.submitted_at);
        }
        found.then(|| min.to_string()..max.to_string())
    }
}
