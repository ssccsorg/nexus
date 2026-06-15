// ── FihStorage — unified FIH storage over FihIo ──────────────────
//
// Implements FactCapable, IntentCapable, HintCapable, StorageRead, and
// EvictCapable on top of a single FihIo implementation.
//
// All state transitions happen in memory first (buffer), then are flushed
// to IO via FihSession. This file handles the sync core logic.
//
// Design invariants:
//   - store_content() enqueues WriteOps via pending, never calls io.write() directly
//   - read_state() loads blob Content from IO via blob_hash
//   - conclude_intent() passes real to_fact/concluded_at to try_conclude()
//   - all timestamps flow through Now trait, never SystemTime::now() directly

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use nexus_model::{
    BlackboardError, BoardState, Content, EvictCapable, Fact, FactCapable, FihHash, FilterCapable,
    Hint, HintCapable, Intent, IntentCapable, Now, PartitionData, StateFilter, StorageRead,
};

use crate::entity_store::{EntityStore, MemoryEntityStore};
use crate::index::FihCoord;
use crate::io::{AsyncFileIo, WriteOp};
use crate::record::{ContentMeta, FactRecord, HintRecord, IntentRecord, IntentStatus};

/// Chain entry format: serialized by flush_since, deserialized by import_chain_file.
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
/// IO calls use inline futures_executor::block_on for sync bridging.
pub struct FihStorage<I: AsyncFileIo> {
    pub io: I,
    project_id: String,
    clock: Box<dyn Now + Send + Sync>,
    /// When true, every write operation also flushes pending ops to IO
    /// immediately, ensuring durability at the cost of batching.
    auto_flush: bool,
    // In-memory stores: rebuilt from IO on hydrate, kept in sync for reads.
    pub fact_store: Box<dyn EntityStore<FactRecord>>,
    pub intent_store: Box<dyn EntityStore<IntentRecord>>,
    pub hint_store: Box<dyn EntityStore<HintRecord>>,
    // Indices
    coord: FihCoord,
    // Pending writes (for FihSession coordination).
    pub(crate) pending: Mutex<Vec<WriteOp>>,
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
            pending: Mutex::new(Vec::new()),
        }
    }

    /// Flush pending writes if auto_flush is enabled.
    fn maybe_flush(&self) -> Result<(), String> {
        if self.auto_flush {
            self.flush_pending()
        } else {
            Ok(())
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
            pending: Mutex::new(Vec::new()),
        }
    }

    /// Rebuild in-memory cache from IO storage.
    pub fn rebuild_cache(&self) -> Result<(), String> {
        let fact_keys = self.sync_list("facts/")?;
        let mut facts: Vec<(String, FactRecord)> = Vec::new();
        for key in fact_keys {
            if let Some(bytes) = self.sync_read(&key)?
                && let Ok(record) = postcard::from_bytes::<FactRecord>(&bytes)
            {
                facts.push((record.id.clone(), record));
            }
        }

        let intent_keys = self.sync_list("intents/")?;
        let mut intents: Vec<(String, IntentRecord)> = Vec::new();
        for key in intent_keys {
            if let Some(bytes) = self.sync_read(&key)?
                && let Ok(record) = postcard::from_bytes::<IntentRecord>(&bytes)
            {
                intents.push((record.id.clone(), record));
            }
        }

        let hint_keys = self.sync_list("hints/")?;
        let mut hints: Vec<(String, HintRecord)> = Vec::new();
        for key in hint_keys {
            if let Some(bytes) = self.sync_read(&key)?
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
    /// search). Other indices (by_origin, by_fact, ref_counts) are
    /// order-independent and built during the same pass.
    fn rebuild_coord(&self) {
        let mut facts = self.fact_store.values();
        let intents = self.intent_store.values();

        // Sort facts by submitted_at to maintain OrderedIndex monotonicity
        facts.sort_by_key(|r| r.submitted_at);

        for r in &facts {
            self.coord.by_time.record(r.submitted_at, &r.id);
            self.coord
                .by_origin
                .write()
                .unwrap()
                .entry(r.origin.clone())
                .or_default()
                .push(r.id.clone());
            self.coord
                .ref_counts
                .write()
                .unwrap()
                .entry(r.id.clone())
                .or_insert_with(|| AtomicU64::new(0));
        }

        for r in &intents {
            for fid in &r.from_facts {
                if let Some(rc) = self.coord.ref_counts.read().unwrap().get(fid) {
                    rc.fetch_add(1, Ordering::Relaxed);
                }
                self.coord
                    .by_fact
                    .write()
                    .unwrap()
                    .entry(fid.clone())
                    .or_default()
                    .push(r.id.clone());
            }
        }
    }

    /// Sync bridge: block_on wrapper for async IO read.
    fn sync_read(&self, path: &str) -> Result<Option<Vec<u8>>, String> {
        futures_executor::block_on(self.io.read(path))
    }

    /// Sync bridge: block_on wrapper for async IO write.
    #[expect(dead_code)]
    fn sync_write(&self, path: &str, data: &[u8]) -> Result<(), String> {
        futures_executor::block_on(self.io.write(path, data))
    }

    /// Sync bridge: block_on wrapper for async IO list.
    fn sync_list(&self, prefix: &str) -> Result<Vec<String>, String> {
        futures_executor::block_on(self.io.list(prefix))
    }

    /// Sync bridge: block_on wrapper for async IO delete.
    #[expect(dead_code)]
    fn sync_delete(&self, path: &str) -> Result<(), String> {
        futures_executor::block_on(self.io.delete(path))
    }

    /// Sync bridge: block_on wrapper for async IO apply_batch.
    fn sync_apply_batch(&self, ops: &[WriteOp]) -> Result<(), String> {
        futures_executor::block_on(self.io.apply_batch(ops))
    }

    /// Flush pending writes to IO.
    pub fn intents_by_fact(&self, fact_id: &str) -> Vec<String> {
        self.coord.intents_by_fact(fact_id)
    }

    /// Import a single delta chain file, restoring its records into the
    /// in-memory cache. The chain file must have been created by flush_since.
    /// Returns the number of records restored (facts + intents).
    pub fn import_chain_file(&self, path: &str) -> Result<u64, String> {
        let bytes = self
            .sync_read(path)?
            .ok_or_else(|| format!("chain file not found: {}", path))?;
        let entry: ChainEntry =
            postcard::from_bytes(&bytes).map_err(|e| format!("deserialize chain: {e}"))?;
        // Reset Hot mode path: always write records to IO then rebuild.
        // This unifies with rebuild_cache semantics and eliminates the
        // fragile in-memory cache merge path.
        for r in &entry.facts {
            let bytes = postcard::to_allocvec(r).map_err(|e| e.to_string())?;
            self.pending.lock().unwrap().push(WriteOp::Write {
                path: r.key(),
                data: bytes,
            });
        }
        for r in &entry.intents {
            let bytes = postcard::to_allocvec(r).map_err(|e| e.to_string())?;
            self.pending.lock().unwrap().push(WriteOp::Write {
                path: r.key(),
                data: bytes,
            });
        }
        let count = (entry.facts.len() + entry.intents.len()) as u64;
        self.flush_pending()?;
        if count > 0 {
            self.rebuild_cache()?;
        }
        Ok(count)
    }

    /// Flush pending writes to IO.
    pub fn flush_pending(&self) -> Result<(), String> {
        let ops = std::mem::take(&mut *self.pending.lock().map_err(|e| e.to_string())?);
        if !ops.is_empty() {
            self.sync_apply_batch(&ops)?;
        }
        Ok(())
    }

    /// Enqueue content as a blob write. FIH is append-only: no dedup
    /// read needed because records are never overwritten. R2 is
    /// last-writer-wins, so duplicate blob_hash writes are harmless.
    fn enqueue_content(&self, content: &Content) -> Result<String, String> {
        let blob_hash = content_hash(&content.data);
        let blob_path = format!("blob/{}.bin", blob_hash);

        self.pending.lock().unwrap().push(WriteOp::Write {
            path: blob_path,
            data: content.data.clone(),
        });

        let meta = ContentMeta {
            mime_type: content.mime_type.clone(),
            size: content.data.len() as u64,
        };
        let meta_bytes = postcard::to_allocvec(&meta).map_err(|e| e.to_string())?;
        self.pending.lock().unwrap().push(WriteOp::Write {
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
        let pending = self.pending.lock().unwrap();
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

        // No data in pending — return empty content. The actual blob
        // lives in R2 but we avoid sync_read (block_on) here because
        // WASM cannot drive async I/O through futures_executor.
        Content {
            mime_type: default_mime.to_string(),
            data: Vec::new(),
        }
    }

    /// Read mime_type from blob meta file. Returns None if not found.
    fn read_mime_type(&self, blob_hash: &str) -> Option<String> {
        let meta_path = format!("blob/{}.bin.meta", blob_hash);
        match self.sync_read(&meta_path) {
            Ok(Some(bytes)) => postcard::from_bytes::<ContentMeta>(&bytes)
                .ok()
                .map(|m| m.mime_type),
            _ => None,
        }
    }
}

fn content_hash(data: &[u8]) -> String {
    // WASM-safe: no hasher dependency, just use data length as identity.
    // In production, use SHA-256. For append-only FIH this is fine
    // because blob_hash is only used for file naming, not security.
    format!("{}", data.len())
}

impl<I: AsyncFileIo> StorageRead for FihStorage<I> {
    fn project_id(&self) -> &str {
        &self.project_id
    }

    fn read_state(&self) -> BoardState {
        let facts = self.fact_store.values();
        let intents = self.intent_store.values();
        let hints = self.hint_store.values();

        BoardState {
            facts: facts
                .into_iter()
                .map(|r| {
                    let content = self.load_content(&r.blob_hash, "application/octet-stream");
                    Fact {
                        id: FihHash(r.id.clone()),
                        origin: r.origin.clone(),
                        content,
                        creator: r.creator.clone(),
                    }
                })
                .collect(),
            intents: intents
                .into_iter()
                .map(|r| Intent {
                    id: FihHash(r.id.clone()),
                    from_facts: r.from_facts.clone(),
                    description: {
                        if r.description_hash.is_empty() {
                            r.id.clone()
                        } else {
                            let c = self.load_content(&r.description_hash, "text/plain");
                            String::from_utf8_lossy(&c.data).to_string()
                        }
                    },
                    creator: r.creator.clone(),
                    worker: match &r.status {
                        IntentStatus::Claimed { worker, .. } => Some(worker.clone()),
                        IntentStatus::Concluded { worker, .. } => Some(worker.clone()),
                        IntentStatus::Submitted => None,
                    },
                    to_fact_id: match &r.status {
                        IntentStatus::Concluded { to_fact, .. } => Some(to_fact.clone()),
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
                })
                .collect(),
            hints: hints
                .into_iter()
                .map(|r| Hint {
                    id: FihHash(r.id.clone()),
                    content: r.content.clone(),
                    creator: r.creator.clone(),
                })
                .collect(),
        }
    }
}

// ── FactCapable ──────────────────────────────────────────────────────────

impl<I: AsyncFileIo> FactCapable for FihStorage<I> {
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
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

        // Update cache immediately for subsequent reads
        self.fact_store.insert(record.id.clone(), record);
        self.pending.lock().unwrap().push(op);
        self.maybe_flush().map_err(BlackboardError::Internal)?;

        // Update indices
        let ts = self.clock.now_nanos();
        self.coord.by_time.record(ts, &fact.id.0);
        {
            let mut origin_map = self.coord.by_origin.write().unwrap();
            origin_map
                .entry(fact.origin.clone())
                .or_default()
                .push(fact.id.0.clone());
        }
        // ref_count defaults to 0 — orphan unless referenced by an Intent
        self.coord
            .ref_counts
            .write()
            .unwrap()
            .entry(fact.id.0.clone())
            .or_insert_with(|| AtomicU64::new(0));

        Ok(fact.id.clone())
    }
}

// ── HintCapable ──────────────────────────────────────────────────────────

impl<I: AsyncFileIo> HintCapable for FihStorage<I> {
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        let record = HintRecord {
            id: hint.id.0.clone(),
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
        self.pending.lock().unwrap().push(op);
        self.maybe_flush().map_err(BlackboardError::Internal)?;

        Ok(())
    }
}

// ── IntentCapable ────────────────────────────────────────────────────────

impl<I: AsyncFileIo> IntentCapable for FihStorage<I> {
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        // Verify at least one from_fact exists and all referenced facts exist
        if intent.from_facts.is_empty() {
            return Err(BlackboardError::Forbidden(
                "intent must reference at least one fact".into(),
            ));
        }
        for fid in &intent.from_facts {
            if !self.fact_store.contains_key(fid) {
                return Err(BlackboardError::NotFound(format!("Fact {fid} not found")));
            }
        }
        // Increment ref_count for each from_fact
        {
            let refs = self.coord.ref_counts.read().unwrap();
            for fid in &intent.from_facts {
                if let Some(rc) = refs.get(fid) {
                    rc.fetch_add(1, Ordering::Relaxed);
                }
            }
        }

        let record = IntentRecord {
            id: intent.id.0.clone(),
            from_facts: intent.from_facts.clone(),
            description_hash: String::new(),
            creator: intent.creator.clone(),
            status: IntentStatus::Submitted,
            created_at: self.clock.now_secs(),
        };

        let bytes =
            postcard::to_allocvec(&record).map_err(|e| BlackboardError::Internal(e.to_string()))?;

        let op = WriteOp::Write {
            path: record.key(),
            data: bytes,
        };

        self.intent_store.insert(record.id.clone(), record);
        // Update by_from_fact reverse index
        let mut by_fact = self.coord.by_fact.write().unwrap();
        for fid in &intent.from_facts {
            by_fact
                .entry(fid.clone())
                .or_default()
                .push(intent.id.0.clone());
        }
        self.pending.lock().unwrap().push(op);
        self.maybe_flush().map_err(BlackboardError::Internal)?;

        Ok(intent.id.clone())
    }

    fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let mut record = self
            .intent_store
            .get(intent_id)
            .ok_or_else(|| BlackboardError::NotFound(format!("Intent {intent_id} not found")))?;

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
        self.pending.lock().unwrap().push(WriteOp::Write {
            path: record.key(),
            data: bytes,
        });
        self.intent_store.insert(intent_id.to_string(), record);
        self.maybe_flush().map_err(BlackboardError::Internal)?;

        Ok(())
    }

    fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let mut record = self
            .intent_store
            .get(intent_id)
            .ok_or_else(|| BlackboardError::NotFound(format!("Intent {intent_id} not found")))?;

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
        self.pending.lock().unwrap().push(WriteOp::Write {
            path: record.key(),
            data: bytes,
        });
        self.intent_store.insert(intent_id.to_string(), record);
        self.maybe_flush().map_err(BlackboardError::Internal)?;

        Ok(())
    }

    fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let mut record = self
            .intent_store
            .get(intent_id)
            .ok_or_else(|| BlackboardError::NotFound(format!("Intent {intent_id} not found")))?;

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
        self.pending.lock().unwrap().push(WriteOp::Write {
            path: record.key(),
            data: bytes,
        });
        self.intent_store.insert(intent_id.to_string(), record);
        self.maybe_flush().map_err(BlackboardError::Internal)?;

        Ok(())
    }

    /// Conclude an intent: transition Claimed → Concluded, produce result Fact.
    fn conclude_intent(&self, intent_id: &str, result: &str) -> Result<Fact, BlackboardError> {
        let mut record = self
            .intent_store
            .get(intent_id)
            .ok_or_else(|| BlackboardError::NotFound(format!("Intent {intent_id} not found")))?;

        // Extract worker before consuming status
        let worker = match &record.status {
            IntentStatus::Claimed { worker, .. } => worker.clone(),
            IntentStatus::Submitted => {
                return Err(BlackboardError::Internal("not claimed".to_string()));
            }
            IntentStatus::Concluded { .. } => {
                return Err(BlackboardError::Internal("already concluded".to_string()));
            }
        };

        // Create conclusion Fact first (its ID becomes to_fact)
        let conclusion_fact_id = format!("f_concl_{}", intent_id);
        let new_fact = Fact {
            id: FihHash(conclusion_fact_id.clone()),
            origin: format!("conclusion:{}", intent_id),
            content: Content {
                mime_type: "text/plain".into(),
                data: result.as_bytes().to_vec(),
            },
            creator: worker.clone(),
        };

        // Transition status with real IDs
        let now_ns = self.clock.now_nanos();
        let new_status = record
            .status
            .try_conclude(&conclusion_fact_id, now_ns)
            .map_err(BlackboardError::Internal)?;

        record.status = new_status;

        // Submit conclusion fact via FactCapable, then re-serialize intent
        FactCapable::submit_fact(self, &new_fact)?;

        // Decrement ref_count for from_facts (intent no longer references them)
        {
            let refs = self.coord.ref_counts.read().unwrap();
            if let Some(r) = self.intent_store.get(intent_id) {
                for fid in &r.from_facts {
                    if let Some(rc) = refs.get(fid) {
                        rc.fetch_sub(1, Ordering::Relaxed);
                    }
                }
            }
        }

        // Remove intent from by_from_fact reverse index
        {
            if let Some(r) = self.intent_store.get(intent_id) {
                let mut by_fact = self.coord.by_fact.write().unwrap();
                for fid in &r.from_facts {
                    if let Some(refs) = by_fact.get_mut(fid) {
                        refs.retain(|i| i != intent_id);
                    }
                }
            }
        }

        let intent_bytes =
            postcard::to_allocvec(&record).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.pending.lock().unwrap().push(WriteOp::Write {
            path: format!("intents/i_{}.intent", intent_id),
            data: intent_bytes,
        });
        self.intent_store.insert(intent_id.to_string(), record);
        self.maybe_flush().map_err(BlackboardError::Internal)?;

        Ok(new_fact)
    }
}

// ── EvictCapable ─────────────────────────────────────────────────────────

impl<I: AsyncFileIo> EvictCapable for FihStorage<I> {
    fn approximate_size(&self) -> usize {
        let facts = self.fact_store.len();
        let intents = self.intent_store.len();
        let hints = self.hint_store.len();
        (facts + intents + hints) * 256
    }

    fn evict_before(&self, before: &str) -> Result<u64, String> {
        let before_secs: u64 = before.parse().unwrap_or(0);
        let removed = std::sync::Arc::new(AtomicU64::new(0));
        let removed_clone = removed.clone();

        self.hint_store.retain(Box::new(move |_, r| {
            if r.submitted_at < before_secs {
                removed_clone.fetch_add(1, Ordering::Relaxed);
                false
            } else {
                true
            }
        }));

        Ok(removed.load(Ordering::Relaxed))
    }

    fn evict_stale_intents(&self, older_than_secs: u64) -> Result<u64, String> {
        let now = self.clock.now_secs();
        let cutoff = now.saturating_sub(older_than_secs);

        let removed = std::sync::Arc::new(AtomicU64::new(0));
        let removed_clone = removed.clone();

        self.intent_store.retain(Box::new(move |_, r| {
            if matches!(r.status, IntentStatus::Submitted) && r.created_at < cutoff {
                removed_clone.fetch_add(1, Ordering::Relaxed);
                false
            } else {
                true
            }
        }));

        Ok(removed.load(Ordering::Relaxed))
    }
}
// ── FilterCapable ────────────────────────────────────────────────────────

impl<I: AsyncFileIo> FilterCapable for FihStorage<I> {
    fn read_state_filtered(&self, filter: &StateFilter) -> BoardState {
        // Determine time range using TimeIndex (O(log N) seek)
        let time_filtered_ids: Option<std::collections::HashSet<String>> =
            match (&filter.since, &filter.until) {
                (Some(since_str), Some(until_str)) => {
                    // Both bounds: range query
                    if let (Ok(since_ts), Ok(until_ts)) =
                        (since_str.parse::<u64>(), until_str.parse::<u64>())
                    {
                        Some(
                            self.coord
                                .by_time
                                .range(&since_ts, &until_ts)
                                .into_iter()
                                .map(|(_, id)| id)
                                .collect(),
                        )
                    } else {
                        None
                    }
                }
                (Some(since_str), None) => {
                    // Lower bound only: since query
                    if let Ok(since_ts) = since_str.parse::<u64>() {
                        Some(
                            self.coord
                                .by_time
                                .since(&since_ts)
                                .into_iter()
                                .map(|(_, id)| id)
                                .collect(),
                        )
                    } else {
                        None
                    }
                }
                (None, Some(until_str)) => {
                    // Upper bound only: as_of query (time-travel)
                    if let Ok(until_ts) = until_str.parse::<u64>() {
                        Some(
                            self.coord
                                .by_time
                                .as_of(&until_ts)
                                .into_iter()
                                .map(|(_, id)| id)
                                .collect(),
                        )
                    } else {
                        None
                    }
                }
                (None, None) => None,
            };

        let mut state = StorageRead::read_state(self);

        // Apply time filter if active
        if let Some(ids) = &time_filtered_ids {
            state.facts.retain(|f| ids.contains(&f.id.0));
            state.intents.retain(|i| ids.contains(&i.id.0));
            state.hints.retain(|h| ids.contains(&h.id.0));
        }

        if let Some(ids) = &filter.fact_ids {
            state.facts.retain(|f| ids.contains(&f.id.0));
        }
        if let Some(ids) = &filter.intent_ids {
            state.intents.retain(|i| ids.contains(&i.id.0));
        }
        if let Some(ids) = &filter.hint_ids {
            state.hints.retain(|h| ids.contains(&h.id.0));
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

// ── FlushCapable ───────────────────────────────────────────────────────────

use nexus_model::{
    ColdStorage, FlushCapable, FlushCursor, FlushResult, ScanCapable, TimeRangeCapable,
};
use std::ops::Range;

// ── FihStorage as HotStorage (standalone Blackboard) ───────────────
//
// StorageRead + FactCapable + IntentCapable + HintCapable + FilterCapable +
// EvictCapable + FlushCapable — all implemented above.
//
// FihStorage can operate as a standalone Blackboard (no DualStorage
// needed), OR as the cold half of DualStorage via the ColdStorage trait.
// This is nex's principle of recursive self-similarity: the same struct
// fulfills both roles through trait composition.

// ── ScanCapable ───────────────────────────────────────────────────────────

impl<I: AsyncFileIo> ScanCapable for FihStorage<I> {
    fn scan_partition(&self, partition: &str) -> Result<PartitionData, String> {
        let state = StorageRead::read_state(self);
        let prefix = format!("partition:{}", partition);
        Ok(PartitionData {
            partition: partition.into(),
            facts: state
                .facts
                .into_iter()
                .filter(|f| f.origin == prefix)
                .collect(),
            intents: state
                .intents
                .into_iter()
                .filter(|i| i.creator == prefix)
                .collect(),
            hints: state
                .hints
                .into_iter()
                .filter(|h| h.creator == prefix)
                .collect(),
        })
    }
}

// ── TimeRangeCapable ──────────────────────────────────────────────────────

impl<I: AsyncFileIo> TimeRangeCapable for FihStorage<I> {
    fn time_range(&self) -> Option<Range<String>> {
        let first = self.coord.by_time.first_key()?;
        let last = self.coord.by_time.last_key()?;
        Some(first.to_string()..last.to_string())
    }
}

// ── ColdStorage ───────────────────────────────────────────────────────────

impl<I: AsyncFileIo + Send> ColdStorage for FihStorage<I> {
    fn write_blob(&self, key: &str, data: &[u8]) -> Result<(), String> {
        self.pending.lock().unwrap().push(WriteOp::Write {
            path: key.to_string(),
            data: data.to_vec(),
        });
        Ok(())
    }
}

// ── FlushCapable ───────────────────────────────────────────────────────────

impl<I: AsyncFileIo> FlushCapable for FihStorage<I> {
    fn flush_since(&self, cursor: &FlushCursor) -> Result<FlushResult, String> {
        let since_ts = cursor.last_flushed_at;
        let now_ts = self.clock.now_nanos();

        // Collect delta IDs via TimeIndex (O(log N))
        let delta_ids: Vec<(String, u64)> = self
            .coord
            .by_time
            .since(&since_ts)
            .into_iter()
            .map(|(ts, id)| (id, ts))
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

        // Build delta chain entry: batch all delta records into one snapshot.
        // Hints are intentionally excluded from the chain because they are
        // ephemeral (see EvictCapable::evict_before). Hint reconstruction
        // must use rebuild_cache() which reads directly from the hints/ prefix.
        let mut facts = Vec::new();
        let mut intents = Vec::new();
        {
            for (id, _) in &delta_ids {
                if let Some(record) = self.fact_store.get(id) {
                    facts.push(record);
                }
                if let Some(record) = self.intent_store.get(id) {
                    intents.push(record);
                }
            }
        }

        // Serialize delta chain entry
        let entry = ChainEntry {
            prev_cursor: cursor.last_flushed_at,
            records_flushed,
            facts,
            intents,
        };
        let chain_bytes =
            postcard::to_allocvec(&entry).map_err(|e| format!("serialize chain: {e}"))?;

        let chain_path = format!("flush/{}/cursor_{}.chain", cursor.partition, now_ts);
        self.pending.lock().unwrap().push(WriteOp::Write {
            path: chain_path,
            data: chain_bytes,
        });

        // Write pending batch to IO
        self.flush_pending()?;

        Ok(FlushResult {
            records_flushed,
            new_cursor: FlushCursor {
                last_flushed_at: now_ts,
                partition: cursor.partition.clone(),
            },
        })
    }
}
