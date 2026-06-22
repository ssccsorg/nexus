// ── FihStorage — unified FIH storage over AsyncFileIo ──────────────────
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

use std::cell::{Cell, RefCell};
use std::ops::Range;

use nexus_model::{
    BlackboardError, BoardState, Content, Fact, FihHash, FlushCursor, FlushResult, Hint, Intent,
    Now, PartitionData, StateFilter,
};

use super::entity_store::{EntityStore, MemoryEntityStore};
use super::index::FihCoord;
use super::record::{ContentMeta, FactRecord, HintRecord, IntentRecord, IntentStatus};
use crate::io::file_io::{AsyncFileIo, WriteOp};
use crate::storage::semantic::fih::FihRecordLoad;
use crate::storage::semantic::record::{Query, RecordLoad};

/// Chain entry format: serialized by flush_since for delta chain files.
/// Named struct avoids postcard tuple field ordering ambiguity with empty vecs.
#[derive(serde::Serialize, serde::Deserialize)]
struct ChainEntry {
    prev_cursor: u64,
    records_flushed: u64,
    facts: Vec<FactRecord>,
    intents: Vec<IntentRecord>,
}

/// Unified FIH storage backended by an abstract IO layer.
///
/// All FIH trait methods are sync. They enqueue WriteOps into a buffer
/// for batch commit by the outer FihSession layer.
/// IO-bound operations (flush_pending, rebuild_cache) are async.
pub struct FihStorage<I: AsyncFileIo> {
    pub io: I,
    project_id: String,
    clock: Box<dyn Now + Send + Sync>,
    /// When true, every write operation also flushes pending ops to IO
    /// immediately, ensuring durability at the cost of batching.
    #[expect(dead_code)]
    auto_flush: bool,
    // In-memory stores: rebuilt from IO on hydrate, kept in sync for reads.
    pub fact_store: Box<dyn EntityStore<FactRecord>>,
    pub intent_store: Box<dyn EntityStore<IntentRecord>>,
    pub hint_store: Box<dyn EntityStore<HintRecord>>,
    // Indices
    coord: FihCoord,
    // Pending writes (for FihSession coordination).
    pub(crate) pending: RefCell<Vec<WriteOp>>,
}

impl<I: AsyncFileIo> FihStorage<I> {
    pub fn new(io: I, project_id: &str) -> Self {
        Self::with_clock(io, project_id, Box::new(nexus_model::SystemClock))
    }

    pub fn with_clock(io: I, project_id: &str, clock: Box<dyn Now + Send + Sync>) -> Self {
        Self::with_clock_and_memory(io, project_id, clock)
    }

    /// Create storage with auto-flush enabled. Every write operation
    /// immediately flushes pending ops to IO for durability.
    /// Useful for R2-backed or direct-write deployments.
    pub fn with_auto_flush(io: I, project_id: &str) -> Self {
        Self::with_all(io, project_id, Box::new(nexus_model::SystemClock), true)
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
            fact_store: Box::new(MemoryEntityStore::<FactRecord>::new()),
            intent_store: Box::new(MemoryEntityStore::<IntentRecord>::new()),
            hint_store: Box::new(MemoryEntityStore::<HintRecord>::new()),
            coord: FihCoord::new(),
            pending: RefCell::new(Vec::new()),
        }
    }

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
            fact_store: Box::new(MemoryEntityStore::<FactRecord>::new()),
            intent_store: Box::new(MemoryEntityStore::<IntentRecord>::new()),
            hint_store: Box::new(MemoryEntityStore::<HintRecord>::new()),
            coord: FihCoord::new(),
            pending: RefCell::new(Vec::new()),
        }
    }

    /// Rebuild in-memory cache from IO storage.
    ///
    /// Tries chain-file replay first (single R2 list + sequential read of
    /// chain files). Falls back to individual fact/intent/hint reads if no
    /// chain files exist (legacy format).
    pub async fn rebuild_cache(&self) -> Result<(), String> {
        // Try chain-file replay first (fast path: fewer reads).
        let chain_keys = self.io.list("flush/").await?;
        if !chain_keys.is_empty() {
            let mut facts: Vec<(String, FactRecord)> = Vec::new();
            let mut intents: Vec<(String, IntentRecord)> = Vec::new();

            // Chain files are prefix-sorted by cursor timestamp.
            // Read them in order and merge into the caches.
            let mut sorted = chain_keys;
            sorted.sort();
            for key in &sorted {
                if let Some(bytes) = self.io.read(key).await?
                    && let Ok(entry) = postcard::from_bytes::<ChainEntry>(&bytes)
                {
                    for r in entry.facts {
                        facts.push((r.id.clone(), r));
                    }
                    for r in entry.intents {
                        intents.push((r.id.clone(), r));
                    }
                }
            }

            self.fact_store.replace_from(facts);
            self.intent_store.replace_from(intents);

            // Hints are not stored in chain files (ephemeral).
            // Read them individually (typically few hints).
            let hint_keys = self.io.list("hints/").await?;
            let mut hints: Vec<(String, HintRecord)> = Vec::new();
            for key in hint_keys {
                if let Some(bytes) = self.io.read(&key).await?
                    && let Ok(record) = postcard::from_bytes::<HintRecord>(&bytes)
                {
                    hints.push((record.id.clone(), record));
                }
            }
            self.hint_store.replace_from(hints);

            self.rebuild_coord();
            return Ok(());
        }

        // Legacy path: read individual fact/intent/hint files.
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

        self.fact_store.replace_from(facts);
        self.intent_store.replace_from(intents);
        self.hint_store.replace_from(hints);

        self.rebuild_coord();

        Ok(())
    }

    /// Rebuild FihCoord indices from current EntityStore contents.
    ///
    /// Records are sorted by submitted_at before insertion to guarantee
    /// monotonic ordering in by_time (required by OrderedIndex's binary
    /// search). Other indices are order-independent and built during the
    /// same pass via FihCoord methods.
    fn rebuild_coord(&self) {
        self.coord.clear();

        let mut facts = self.fact_store.values();
        let intents = self.intent_store.values();

        // Sort facts by submitted_at to maintain OrderedIndex monotonicity
        facts.sort_by_key(|r| r.submitted_at);

        for r in &facts {
            let id_bytes = FihHash::from_hex(&r.id);
            let idx = self.coord.intern(&id_bytes.0);
            self.coord.by_time.record(r.submitted_at, idx);
            self.coord
                .record_fact(&id_bytes.0, &r.origin, &r.creator, r.submitted_at);
        }

        for r in &intents {
            let id_bytes = FihHash::from_hex(&r.id);
            let from_bytes: Vec<[u8; 32]> = r
                .from_facts
                .iter()
                .map(|f| FihHash::from_hex(f).0)
                .collect();
            self.coord
                .record_intent(&id_bytes.0, &r.creator, r.created_at, &from_bytes);
        }
    }

    /// Flush pending writes to IO.
    /// Rebuild semantic stores (BM25, Vectorize buffer) from fact_store after rebuild_cache.
    /// Reads content blobs from IO and inserts text into all registered semantic stores.
    pub async fn rebuild_semantic(&self) -> Result<(), String> {
        struct TextRecord {
            text: String,
        }
        impl crate::storage::semantic::record::RecordLoad for TextRecord {
            fn content(&self, _id: u32) -> Option<Vec<u8>> {
                Some(self.text.as_bytes().to_vec())
            }
            fn features(&self, _id: u32) -> Option<Vec<f32>> {
                None
            }
        }

        let facts = self.fact_store.values();
        for r in facts {
            let content = load_blob(&self.io, &r.blob_hash).await;
            if content.data.is_empty() {
                continue;
            }
            let text = String::from_utf8_lossy(&content.data).to_string();
            if text.trim().is_empty() {
                continue;
            }
            let id_bytes = nexus_model::FihHash::from_hex(&r.id);
            let idx = self.coord.intern(&id_bytes.0);
            let load = TextRecord { text };
            self.semantic_insert(idx, &load).await.ok();
        }
        Ok(())
    }

    pub async fn flush_pending(&self) -> Result<(), String> {
        let ops = std::mem::take(&mut *self.pending.try_borrow_mut().map_err(|e| e.to_string())?);
        if !ops.is_empty() {
            self.io.apply_batch(&ops).await?;
        }
        Ok(())
    }

    /// Flush pending writes and write a chain-file checkpoint.
    ///
    /// Unlike plain flush_pending(), this also writes a chain file
    /// (flush/{partition}/cursor_{timestamp}.chain) containing all
    /// fact and intent records. The chain file enables fast cold-start
    /// recovery via rebuild_cache() — one R2 list + sequential reads
    /// instead of N individual GET requests.
    ///
    /// Call this periodically (e.g., after bulk ingest) to keep
    /// chain files current. Plain flush_pending() is sufficient for
    /// most individual writes; call flush_with_chain() before
    /// deployments or after batch operations.
    pub async fn flush_with_chain(&self) -> Result<(), String> {
        self.flush_pending().await?;
        let now_ts = self.clock.now_nanos();
        let facts = self.fact_store.values();
        let intents = self.intent_store.values();
        let entry = ChainEntry {
            prev_cursor: 0,
            records_flushed: facts.len() as u64,
            facts,
            intents,
        };
        let chain_bytes =
            postcard::to_allocvec(&entry).map_err(|e| format!("serialize chain: {e}"))?;
        let chain_path = format!("flush/default/cursor_{}.chain", now_ts);
        self.io
            .write(&chain_path, &chain_bytes)
            .await
            .map_err(|e| format!("write chain: {e}"))?;
        Ok(())
    }

    /// Register a semantic store for auto-indexing on fact submission.
    pub fn register_semantic_store(&self, store: Box<dyn crate::storage::semantic::SemanticStore>) {
        self.coord.by_semantic.borrow_mut().push(store);
    }

    /// Access the semantic stores list (for downcasting to concrete types).
    pub fn semantic_stores(
        &self,
    ) -> impl std::ops::Deref<Target = Vec<Box<dyn crate::storage::semantic::SemanticStore>>> {
        self.coord.by_semantic.borrow()
    }

    /// Search semantic stores with the given query.
    pub async fn semantic_search(
        &self,
        query: &dyn Query,
        top_k: usize,
    ) -> Result<Vec<(u32, f32)>, String> {
        self.coord.semantic_search(query, top_k).await
    }

    /// Insert a record into semantic stores with the given load handle.
    pub async fn semantic_insert(&self, id: u32, load: &dyn RecordLoad) -> Result<(), String> {
        self.coord.semantic_insert(id, load).await
    }

    /// Query intents that reference a given fact.
    /// Returns Vec<String> (hex IDs). Each call allocates O(k) strings
    /// where k is the number of referencing intents — acceptable for
    /// expected fan-out sizes (< 100).
    /// Query intents that reference a given fact.
    pub fn intents_by_fact(&self, fact_id: &str) -> Vec<String> {
        let fidx = self.coord.intern_str(fact_id);
        self.coord
            .intents_by_fact(fidx)
            .into_iter()
            .map(|idx| self.coord.resolve(idx))
            .collect()
    }

    /// Resolve a semantic index back to its hex ID string.
    pub fn resolve_semantic_idx(&self, idx: u32) -> String {
        self.coord.resolve(idx)
    }

    /// Enqueue content as a blob write. FIH is append-only: no dedup
    /// read needed because records are never overwritten. R2 is
    /// last-writer-wins, so duplicate blob_hash writes are harmless.
    fn enqueue_content(&self, content: &Content) -> Result<String, String> {
        let blob_hash = content_hash(&content.data);
        let blob_path = format!("blob/{}.bin", blob_hash);

        // Check pending buffer first to avoid duplicate PUTs.
        // Cheap: linear scan over pending ops (typically < 100).
        if self
            .pending
            .borrow()
            .iter()
            .any(|op| matches!(op, WriteOp::Write { path, .. } if *path == blob_path))
        {
            return Ok(blob_hash);
        }

        self.pending.borrow_mut().push(WriteOp::Write {
            path: blob_path,
            data: content.data.clone(),
        });

        let meta = ContentMeta {
            mime_type: content.mime_type.clone(),
            size: content.data.len() as u64,
        };
        let meta_bytes = postcard::to_allocvec(&meta).map_err(|e| e.to_string())?;
        self.pending.borrow_mut().push(WriteOp::Write {
            path: format!("blob/{}.bin.meta", blob_hash),
            data: meta_bytes,
        });

        Ok(blob_hash)
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

fn content_hash(data: &[u8]) -> String {
    // SHA-256 content hash. WASM-compatible (sha2 crate works on wasm32).
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

/// Load a content blob from IO by hash. Returns empty Content if not found.
async fn load_blob(io: &impl AsyncFileIo, blob_hash: &str) -> Content {
    if blob_hash.is_empty() {
        return Content {
            mime_type: "application/json".into(),
            data: Vec::new(),
        };
    }
    let key = format!("blob/{}.bin", blob_hash);
    match io.read(&key).await {
        Ok(Some(data)) => Content {
            mime_type: "application/json".into(),
            data,
        },
        _ => Content {
            mime_type: "application/json".into(),
            data: Vec::new(),
        },
    }
}

// ── FihStorage as RecordLoad ────────────────────────────────────────
//
// Implements the pure semantic RecordLoad trait for SemanticStore
// implementations. FihStorage has access to both the in-memory
// EntityStore (for fact/intent/hint records) and the coord index (for
// ID resolution), making it the natural RecordLoad provider.

impl<I: AsyncFileIo> RecordLoad for FihStorage<I> {
    fn content(&self, id: u32) -> Option<Vec<u8>> {
        let id_str = self.coord.resolve(id);
        if id_str.is_empty() {
            return None;
        }
        let record = self.fact_store.get(&id_str)?;
        let content = self.load_content(&record.blob_hash, "application/octet-stream");
        if content.data.is_empty() {
            None
        } else {
            Some(content.data)
        }
    }

    fn features(&self, _id: u32) -> Option<Vec<f32>> {
        // Feature vectors are not stored in FihStorage directly.
        // External embedding services should set up RecordLoad wrappers.
        None
    }
}

// ── FihStorage as FihRecordLoad ────────────────────────────────────
//
// Extends RecordLoad with FIH-specific accessors for origin and creator.

impl<I: AsyncFileIo> FihRecordLoad for FihStorage<I> {
    fn origin(&self, id: u32) -> Option<String> {
        let id_str = self.coord.resolve(id);
        if id_str.is_empty() {
            return None;
        }
        let record = self.fact_store.get(&id_str)?;
        Some(record.origin.clone())
    }

    fn creator(&self, id: u32) -> Option<String> {
        let id_str = self.coord.resolve(id);
        if id_str.is_empty() {
            return None;
        }
        let record = self.fact_store.get(&id_str)?;
        Some(record.creator.clone())
    }
}

// ── AsyncStorageRead ───────────────────────────────────────────────────────

impl<I: AsyncFileIo> nexus_model::AsyncStorageRead for FihStorage<I> {
    fn project_id(&self) -> &str {
        &self.project_id
    }

    async fn read_state(&self) -> BoardState {
        // Flush any pending writes so IO reflects the latest state.
        let _ = self.flush_pending().await;

        // Direct async IO: list + read from backing store, no block_on.
        let mut facts = Vec::new();
        if let Ok(keys) = self.io.list("facts/").await {
            for key in &keys {
                if let Ok(Some(bytes)) = self.io.read(key).await
                    && let Ok(r) = postcard::from_bytes::<FactRecord>(&bytes)
                {
                    let content = load_blob(&self.io, &r.blob_hash).await;
                    facts.push(Fact {
                        id: FihHash::from_hex(&r.id),
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
                        id: FihHash::from_hex(&r.id),
                        from_facts: r.from_facts.iter().map(|s| FihHash::from_hex(s)).collect(),
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
                                Some(FihHash::from_hex(to_fact))
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
                        id: FihHash::from_hex(&r.id),
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

impl<I: AsyncFileIo> nexus_model::AsyncFactCapable for FihStorage<I> {
    async fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        // Enqueue blob content and fact record in pending buffer only.
        // No direct io.write() — caller must call flush_pending() for durability.
        // This enables batch writes (N paragraphs → 1 apply_batch instead of 2N R2 PUTs).
        let blob_hash = self
            .enqueue_content(&fact.content)
            .map_err(BlackboardError::Internal)?;

        let record = FactRecord::from_model(fact, blob_hash, self.clock.now_nanos());
        let bytes =
            postcard::to_allocvec(&record).map_err(|e| BlackboardError::Internal(e.to_string()))?;

        let op = WriteOp::Write {
            path: record.key(),
            data: bytes,
        };

        // Update in-memory cache immediately for subsequent reads
        self.fact_store.insert(record.id.clone(), record);
        self.pending.borrow_mut().push(op);

        // Update indices via FihCoord (record_fact records by_time internally)
        let ts = self.clock.now_nanos();
        self.coord
            .record_fact(&fact.id.0, &fact.origin, &fact.creator, ts);

        // Auto-index into semantic stores (skip conclusion facts to reduce noise)
        if !fact.origin.starts_with("conclusion:") {
            let fact_idx = self.coord.intern(&fact.id.0);
            self.coord.semantic_insert(fact_idx, self).await.ok();
        }

        Ok(fact.id)
    }
}

// ── AsyncHintCapable ───────────────────────────────────────────────────────

impl<I: AsyncFileIo> nexus_model::AsyncHintCapable for FihStorage<I> {
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
        self.hint_store.insert(record.id.clone(), record);
        self.pending.borrow_mut().push(op);
        Ok(())
    }
}

// ── AsyncIntentCapable ─────────────────────────────────────────────────────

impl<I: AsyncFileIo> nexus_model::AsyncIntentCapable for FihStorage<I> {
    async fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        if intent.from_facts.is_empty() {
            return Err(BlackboardError::Forbidden(
                "intent must reference at least one fact".into(),
            ));
        }
        for fid in &intent.from_facts {
            let fid_str = fid.to_string();
            if !self.fact_store.contains_key(&fid_str) {
                return Err(BlackboardError::NotFound(format!(
                    "Fact {fid_str} not found"
                )));
            }
        }

        let record = super::record::IntentRecord {
            id: intent.id.to_string(),
            from_facts: intent.from_facts.iter().map(|f| f.to_string()).collect(),
            description_hash: String::new(),
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

        // Record intent in coordinator (handles by_fact, ref_counts, by_status, by_creator)
        self.coord.record_intent(
            &intent.id.0,
            &intent.creator,
            record.created_at,
            &intent.from_facts.iter().map(|f| f.0).collect::<Vec<_>>(),
        );

        self.intent_store.insert(record.id.clone(), record);
        self.pending.borrow_mut().push(op);
        Ok(intent.id)
    }

    async fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let _ = self.flush_pending().await;
        let normalized = FihHash::from_hex(intent_id).to_string();
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
        self.io
            .write(&key, &bytes)
            .await
            .map_err(BlackboardError::Internal)?;
        self.intent_store.insert(normalized.clone(), record);
        self.coord
            .update_intent_status(&FihHash::from_hex(&normalized).0, "submitted", "claimed");
        Ok(())
    }

    async fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let _ = self.flush_pending().await;
        let normalized = FihHash::from_hex(intent_id).to_string();
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
        self.io
            .write(&key, &bytes)
            .await
            .map_err(BlackboardError::Internal)?;
        self.intent_store.insert(intent_id.to_string(), record);
        Ok(())
    }

    async fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let _ = self.flush_pending().await;
        let normalized = FihHash::from_hex(intent_id).to_string();
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
        self.io
            .write(&key, &bytes)
            .await
            .map_err(BlackboardError::Internal)?;
        self.intent_store.insert(intent_id.to_string(), record);
        Ok(())
    }

    async fn conclude_intent(
        &self,
        intent_id: &str,
        result: &str,
    ) -> Result<Fact, BlackboardError> {
        let _ = self.flush_pending().await;
        let normalized = FihHash::from_hex(intent_id).to_string();
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
        let new_fact = Fact {
            id: FihHash::from_hex(&conclusion_id),
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

        // Write conclusion fact to R2
        let fact_rec = FactRecord::from_model(&new_fact, String::new(), 0);
        let fact_bytes = postcard::to_allocvec(&fact_rec)
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.io
            .write(&fact_rec.key(), &fact_bytes)
            .await
            .map_err(BlackboardError::Internal)?;
        self.fact_store.insert(fact_rec.id.clone(), fact_rec);

        // Write updated intent to R2
        let intent_bytes =
            postcard::to_allocvec(&record).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.io
            .write(&key, &intent_bytes)
            .await
            .map_err(BlackboardError::Internal)?;
        self.intent_store.insert(intent_id.to_string(), record);
        self.coord
            .update_intent_status(&FihHash::from_hex(intent_id).0, "claimed", "concluded");

        Ok(new_fact)
    }
}

// ── AsyncFilterCapable (in-memory filtering) ────────────────────────────

impl<I: AsyncFileIo> nexus_model::AsyncFilterCapable for FihStorage<I> {
    async fn read_state_filtered(&self, filter: &StateFilter) -> BoardState {
        use std::collections::HashSet;

        // Phase 1: Resolve candidate fact IDs from indexes
        let has_fact_index = filter.creator.is_some()
            || filter.since.is_some()
            || filter.until.is_some()
            || filter.fact_ids.is_some();

        let fact_candidates: Option<HashSet<u32>> = if has_fact_index {
            let mut c: Option<HashSet<u32>> = None;

            if let Some(creator) = &filter.creator {
                let ids: HashSet<u32> = self.coord.facts_by_creator(creator).into_iter().collect();
                c = Some(match c {
                    Some(existing) => existing.intersection(&ids).copied().collect(),
                    None => ids,
                });
            }

            if filter.since.is_some() || filter.until.is_some() {
                let since_ns = filter
                    .since
                    .as_ref()
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(0);
                let until_ns = filter
                    .until
                    .as_ref()
                    .and_then(|u| u.parse::<u64>().ok())
                    .unwrap_or(u64::MAX);
                let time_ids: HashSet<u32> = match (&filter.since, &filter.until) {
                    (Some(_), Some(_)) => self
                        .coord
                        .by_time
                        .range(&since_ns, &until_ns)
                        .into_iter()
                        .map(|(_, idx)| idx)
                        .collect(),
                    (Some(_), None) => self
                        .coord
                        .by_time
                        .since(&since_ns)
                        .into_iter()
                        .map(|(_, idx)| idx)
                        .collect(),
                    (None, Some(_)) => self
                        .coord
                        .by_time
                        .as_of(&until_ns)
                        .into_iter()
                        .map(|(_, idx)| idx)
                        .collect(),
                    (None, None) => unreachable!(),
                };
                c = Some(match c {
                    Some(existing) => existing.intersection(&time_ids).copied().collect(),
                    None => time_ids,
                });
            }

            if let Some(ids) = &filter.fact_ids {
                let id_set: HashSet<u32> = ids.iter().map(|id| self.coord.intern_str(id)).collect();
                c = Some(match c {
                    Some(existing) => existing.intersection(&id_set).copied().collect(),
                    None => id_set,
                });
            }

            c
        } else {
            None
        };

        // Phase 2: Resolve candidate intent IDs from indexes
        let has_intent_index =
            filter.creator.is_some() || filter.status.is_some() || filter.intent_ids.is_some();

        let intent_candidates: Option<HashSet<u32>> = if has_intent_index {
            let mut c: Option<HashSet<u32>> = None;

            if let Some(status) = &filter.status {
                let ids: HashSet<u32> = self.coord.intents_by_status(status).into_iter().collect();
                c = Some(match c {
                    Some(existing) => existing.intersection(&ids).copied().collect(),
                    None => ids,
                });
            }

            if let Some(creator) = &filter.creator {
                let ids: HashSet<u32> = self.coord.facts_by_creator(creator).into_iter().collect();
                c = Some(match c {
                    Some(existing) => existing.intersection(&ids).copied().collect(),
                    None => ids,
                });
            }

            if let Some(ids) = &filter.intent_ids {
                let id_set: HashSet<u32> = ids.iter().map(|id| self.coord.intern_str(id)).collect();
                c = Some(match c {
                    Some(existing) => existing.intersection(&id_set).copied().collect(),
                    None => id_set,
                });
            }

            c
        } else {
            None
        };

        // Phase 3: Selective materialization
        let all_facts = self.fact_store.values();
        let all_intents = self.intent_store.values();
        let all_hints = self.hint_store.values();

        let facts: Vec<Fact> = match fact_candidates {
            Some(ids) => all_facts
                .into_iter()
                .filter(|r| ids.contains(&self.coord.intern_str(&r.id)))
                .map(|r| {
                    let content = self.load_content(&r.blob_hash, "application/octet-stream");
                    Fact {
                        id: FihHash::from_hex(&r.id),
                        origin: r.origin,
                        content,
                        creator: r.creator,
                    }
                })
                .collect(),
            None => all_facts
                .into_iter()
                .map(|r| {
                    let content = self.load_content(&r.blob_hash, "application/octet-stream");
                    Fact {
                        id: FihHash::from_hex(&r.id),
                        origin: r.origin,
                        content,
                        creator: r.creator,
                    }
                })
                .collect(),
        };

        let intents: Vec<Intent> = match intent_candidates {
            Some(ids) => all_intents
                .into_iter()
                .filter(|r| ids.contains(&self.coord.intern_str(&r.id)))
                .map(|r| {
                    let description = if r.description_hash.is_empty() {
                        r.id.clone()
                    } else {
                        let c = self.load_content(&r.description_hash, "text/plain");
                        String::from_utf8_lossy(&c.data).to_string()
                    };
                    Intent {
                        id: FihHash::from_hex(&r.id),
                        from_facts: r.from_facts.iter().map(|s| FihHash::from_hex(s)).collect(),
                        description,
                        creator: r.creator,
                        worker: match &r.status {
                            IntentStatus::Claimed { worker, .. }
                            | IntentStatus::Concluded { worker, .. } => Some(worker.clone()),
                            IntentStatus::Submitted => None,
                        },
                        to_fact_id: match &r.status {
                            IntentStatus::Concluded { to_fact, .. } => {
                                Some(FihHash::from_hex(to_fact))
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
            None => all_intents
                .into_iter()
                .map(|r| {
                    let description = if r.description_hash.is_empty() {
                        r.id.clone()
                    } else {
                        let c = self.load_content(&r.description_hash, "text/plain");
                        String::from_utf8_lossy(&c.data).to_string()
                    };
                    Intent {
                        id: FihHash::from_hex(&r.id),
                        from_facts: r.from_facts.iter().map(|s| FihHash::from_hex(s)).collect(),
                        description,
                        creator: r.creator,
                        worker: match &r.status {
                            IntentStatus::Claimed { worker, .. }
                            | IntentStatus::Concluded { worker, .. } => Some(worker.clone()),
                            IntentStatus::Submitted => None,
                        },
                        to_fact_id: match &r.status {
                            IntentStatus::Concluded { to_fact, .. } => {
                                Some(FihHash::from_hex(to_fact))
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
        };

        let hints: Vec<Hint> = {
            let has_hint_filter = filter.hint_ids.is_some();
            if has_hint_filter {
                let hint_ids_set: Option<HashSet<String>> = filter.hint_ids.as_ref().map(|ids| {
                    ids.iter()
                        .map(|id| FihHash::from_hex(id).to_string())
                        .collect()
                });
                match hint_ids_set {
                    Some(ids) => all_hints
                        .into_iter()
                        .filter(|r| ids.contains(&r.id))
                        .map(|r| Hint {
                            id: FihHash::from_hex(&r.id),
                            content: r.content,
                            creator: r.creator,
                        })
                        .collect(),
                    None => all_hints
                        .into_iter()
                        .map(|r| Hint {
                            id: FihHash::from_hex(&r.id),
                            content: r.content,
                            creator: r.creator,
                        })
                        .collect(),
                }
            } else {
                all_hints
                    .into_iter()
                    .map(|r| Hint {
                        id: FihHash::from_hex(&r.id),
                        content: r.content,
                        creator: r.creator,
                    })
                    .collect()
            }
        };

        // Phase 4: Post-filtering
        let mut state = BoardState {
            facts,
            intents,
            hints,
        };

        let has_intent_time_filter = filter.since.is_some() || filter.until.is_some();
        if has_intent_time_filter {
            let since_ns = filter
                .since
                .as_ref()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let until_ns = filter
                .until
                .as_ref()
                .and_then(|u| u.parse::<u64>().ok())
                .unwrap_or(u64::MAX);
            state.intents.retain(|i| {
                let created_ns = i.created_at.unwrap_or(0) * 1_000_000_000;
                created_ns > since_ns && created_ns <= until_ns
            });
        }

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

impl<I: AsyncFileIo> nexus_model::AsyncEvictCapable for FihStorage<I> {
    async fn approximate_size(&self) -> usize {
        let facts = self.fact_store.len();
        let intents = self.intent_store.len();
        let hints = self.hint_store.len();
        (facts + intents + hints) * 256
    }

    async fn evict_before(&self, before: &str) -> Result<u64, String> {
        let before_secs: u64 = before.parse().unwrap_or(0);
        let removed = std::rc::Rc::new(Cell::new(0u64));
        let removed_clone = std::rc::Rc::clone(&removed);

        self.hint_store.retain(Box::new(move |_, r| {
            if r.submitted_at < before_secs {
                removed_clone.set(removed_clone.get() + 1);
                false
            } else {
                true
            }
        }));

        Ok(removed.get())
    }

    async fn evict_stale_intents(&self, older_than_secs: u64) -> Result<u64, String> {
        let now = self.clock.now_secs();
        let cutoff = now.saturating_sub(older_than_secs);

        let removed = std::rc::Rc::new(Cell::new(0u64));
        let removed_clone = std::rc::Rc::clone(&removed);

        self.intent_store.retain(Box::new(move |_, r| {
            if matches!(r.status, IntentStatus::Submitted) && r.created_at < cutoff {
                removed_clone.set(removed_clone.get() + 1);
                false
            } else {
                true
            }
        }));

        Ok(removed.get())
    }
}

// ── AsyncScanCapable (in-memory scan) ───────────────────────────────────

impl<I: AsyncFileIo> nexus_model::AsyncScanCapable for FihStorage<I> {
    async fn scan_partition(&self, partition: &str) -> Result<PartitionData, String> {
        let facts = self.fact_store.values();
        let intents = self.intent_store.values();
        let hints = self.hint_store.values();

        let prefix = format!("partition:{}", partition);
        Ok(PartitionData {
            partition: partition.into(),
            facts: facts
                .into_iter()
                .filter(|f| f.origin == prefix)
                .map(|r| {
                    let content = self.load_content(&r.blob_hash, "application/octet-stream");
                    Fact {
                        id: FihHash::from_hex(&r.id),
                        origin: r.origin,
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
                        id: FihHash::from_hex(&r.id),
                        from_facts: r.from_facts.iter().map(|s| FihHash::from_hex(s)).collect(),
                        description,
                        creator: r.creator,
                        worker: match &r.status {
                            IntentStatus::Claimed { worker, .. }
                            | IntentStatus::Concluded { worker, .. } => Some(worker.clone()),
                            IntentStatus::Submitted => None,
                        },
                        to_fact_id: match &r.status {
                            IntentStatus::Concluded { to_fact, .. } => {
                                Some(FihHash::from_hex(to_fact))
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
                    id: FihHash::from_hex(&r.id),
                    content: r.content,
                    creator: r.creator,
                })
                .collect(),
        })
    }
}

// ── AsyncTimeRangeCapable (in-memory time range) ────────────────────────

impl<I: AsyncFileIo> nexus_model::AsyncTimeRangeCapable for FihStorage<I> {
    async fn time_range(&self) -> Option<Range<String>> {
        let first = self.coord.by_time.first_key()?;
        let last = self.coord.by_time.last_key()?;
        Some(first.to_string()..last.to_string())
    }
}

// ── AsyncFlushCapable (IO: flush_pending via await) ──────────────────────

impl<I: AsyncFileIo> nexus_model::AsyncFlushCapable for FihStorage<I> {
    async fn flush_since(&self, cursor: &FlushCursor) -> Result<FlushResult, String> {
        let since_ts = cursor.last_flushed_at;
        let now_ts = self.clock.now_nanos();

        let delta_ids: Vec<(String, u64)> = self
            .coord
            .by_time
            .since(&since_ts)
            .into_iter()
            .map(|(_ts, idx)| (self.coord.resolve(idx), _ts))
            .collect();
        let records_flushed = delta_ids.len() as u64;

        if records_flushed == 0 {
            return Ok(FlushResult {
                records_flushed: 0,
                new_cursor: FlushCursor {
                    last_flushed_at: now_ts,
                    partition: cursor.partition.clone(),
                },
            });
        }

        let mut facts = Vec::new();
        let mut intents = Vec::new();
        for (id, _) in &delta_ids {
            if let Some(record) = self.fact_store.get(id) {
                facts.push(record);
            }
            if let Some(record) = self.intent_store.get(id) {
                intents.push(record);
            }
        }

        let entry = ChainEntry {
            prev_cursor: cursor.last_flushed_at,
            records_flushed,
            facts,
            intents,
        };
        let chain_bytes =
            postcard::to_allocvec(&entry).map_err(|e| format!("serialize chain: {e}"))?;

        let chain_path = format!("flush/{}/cursor_{}.chain", cursor.partition, now_ts);
        self.pending.borrow_mut().push(WriteOp::Write {
            path: chain_path,
            data: chain_bytes,
        });

        self.flush_pending().await?;

        Ok(FlushResult {
            records_flushed,
            new_cursor: FlushCursor {
                last_flushed_at: now_ts,
                partition: cursor.partition.clone(),
            },
        })
    }
}
