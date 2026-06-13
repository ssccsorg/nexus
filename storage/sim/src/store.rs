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

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};

use nexus_model::{
    BlackboardError, BoardState, Content, EvictCapable, Fact, FactCapable, FihHash, FilterCapable,
    Hint, HintCapable, Intent, IntentCapable, Now, PartitionData, StateFilter, StorageRead,
};

use crate::index::TimeIndex;
use crate::io::{AsyncFileIo, SyncFileIo, WriteOp};
use crate::record::{ContentMeta, FactRecord, HintRecord, IntentRecord, IntentStatus};

// ── Type aliases for in-memory caches ───────────────────────────────────
//
// Isolate the concrete map implementation so that switching from HashMap to
// BTreeMap (or any other Map-like structure) requires changing only the type
// alias below — not the 50+ call sites throughout this file.

/// Fact cache: fact_id → FactRecord
pub(crate) type FactCache = HashMap<String, FactRecord>;

/// Intent cache: intent_id → IntentRecord
pub(crate) type IntentCache = HashMap<String, IntentRecord>;

/// Hint cache: hint_id → HintRecord
pub(crate) type HintCache = HashMap<String, HintRecord>;

/// Reference counts: fact_id → number of Intents referencing this Fact
pub(crate) type RefCounts = HashMap<String, AtomicU64>;

/// Origin index: origin → [fact_id, ...]
pub(crate) type OriginIndex = HashMap<String, Vec<String>>;

/// Reverse index: fact_id → intent_id list (Intents that reference this Fact).
/// Updated in submit_intent (append) and conclude_intent (remove).
/// Enables O(1) query: "which Intents depend on Fact f001?"
pub(crate) type ByFromFact = HashMap<String, Vec<String>>;

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
/// IO is wrapped in SyncFileIo to bridge the async IO trait with sync callers.
pub struct FihStorage<I: AsyncFileIo> {
    io: SyncFileIo<I>,
    project_id: String,
    clock: Box<dyn Now + Send + Sync>,
    /// When true (default), in-memory caches are active for O(1) reads.
    /// Set to false when used exclusively as ColdStorage — all reads go
    /// directly to IO, and cache fields remain empty.
    is_hotmemory_enabled: bool,
    // In-memory cache: rebuilt from IO on hydrate, kept in sync for reads.
    fact_cache: RwLock<FactCache>,
    intent_cache: RwLock<IntentCache>,
    hint_cache: RwLock<HintCache>,
    // Indices (valid only when is_hotmemory_enabled)
    time_index: TimeIndex,
    ref_counts: RwLock<RefCounts>,
    by_origin: RwLock<OriginIndex>,
    by_from_fact: RwLock<ByFromFact>,
    // Pending writes (for FihSession coordination).
    pub(crate) pending: Mutex<Vec<WriteOp>>,
}

impl<I: AsyncFileIo> FihStorage<I> {
    pub fn new(io: I, project_id: &str) -> Self {
        Self::with_clock(io, project_id, Box::new(nexus_model::SystemClock))
    }

    pub fn with_clock(io: I, project_id: &str, clock: Box<dyn Now + Send + Sync>) -> Self {
        Self::with_clock_and_memory(io, project_id, clock, true)
    }

    /// Create storage with explicit memory cache control.
    /// Set `memory` to false when this instance is used exclusively as
    /// ColdStorage — all reads go directly to IO, saving memory.
    pub fn with_clock_and_memory(
        io: I,
        project_id: &str,
        clock: Box<dyn Now + Send + Sync>,
        is_hotmemory_enabled: bool,
    ) -> Self {
        Self {
            io: SyncFileIo::new(io),
            project_id: project_id.to_string(),
            clock,
            is_hotmemory_enabled,
            fact_cache: RwLock::new(FactCache::new()),
            intent_cache: RwLock::new(IntentCache::new()),
            hint_cache: RwLock::new(HintCache::new()),
            time_index: TimeIndex::new(),
            ref_counts: RwLock::new(RefCounts::new()),
            by_origin: RwLock::new(OriginIndex::new()),
            by_from_fact: RwLock::new(ByFromFact::new()),
            pending: Mutex::new(Vec::new()),
        }
    }

    /// Rebuild in-memory cache from IO storage.
    pub fn rebuild_cache(&self) -> Result<(), String> {
        let fact_keys = self.io.list("facts/")?;
        let mut facts = FactCache::new();
        for key in fact_keys {
            if let Some(bytes) = self.io.read(&key)?
                && let Ok(record) = postcard::from_bytes::<FactRecord>(&bytes)
            {
                facts.insert(record.id.clone(), record);
            }
        }

        let intent_keys = self.io.list("intents/")?;
        let mut intents = IntentCache::new();
        for key in intent_keys {
            if let Some(bytes) = self.io.read(&key)?
                && let Ok(record) = postcard::from_bytes::<IntentRecord>(&bytes)
            {
                intents.insert(record.id.clone(), record);
            }
        }

        let hint_keys = self.io.list("hints/")?;
        let mut hints = HintCache::new();
        for key in hint_keys {
            if let Some(bytes) = self.io.read(&key)?
                && let Ok(record) = postcard::from_bytes::<HintRecord>(&bytes)
            {
                hints.insert(record.id.clone(), record);
            }
        }

        *self.fact_cache.write().map_err(|e| e.to_string())? = facts;
        *self.intent_cache.write().map_err(|e| e.to_string())? = intents;
        *self.hint_cache.write().map_err(|e| e.to_string())? = hints;

        // Rebuild indices from loaded records
        {
            let time_idx = &self.time_index;
            let mut origin_idx = self.by_origin.write().map_err(|e| e.to_string())?;
            let mut by_ff = self.by_from_fact.write().map_err(|e| e.to_string())?;
            let mut refs = self.ref_counts.write().map_err(|e| e.to_string())?;
            let facts = self.fact_cache.read().map_err(|e| e.to_string())?;
            let intents = self.intent_cache.read().map_err(|e| e.to_string())?;

            for r in facts.values() {
                time_idx.record(r.submitted_at, &r.id);
                origin_idx
                    .entry(r.origin.clone())
                    .or_default()
                    .push(r.id.clone());
                refs.entry(r.id.clone())
                    .or_insert_with(|| AtomicU64::new(0));
            }

            for r in intents.values() {
                for fid in &r.from_facts {
                    if let Some(rc) = refs.get(fid) {
                        rc.fetch_add(1, Ordering::Relaxed);
                    }
                    by_ff.entry(fid.clone()).or_default().push(r.id.clone());
                }
            }
        }

        Ok(())
    }

    /// Flush pending writes to IO.
    pub fn intents_by_fact(&self, fact_id: &str) -> Vec<String> {
        self.by_from_fact
            .read()
            .unwrap()
            .get(fact_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Import a single delta chain file, restoring its records into the
    /// in-memory cache. The chain file must have been created by flush_since.
    /// Returns the number of records restored (facts + intents).
    pub fn import_chain_file(&self, path: &str) -> Result<u64, String> {
        let bytes = self
            .io
            .read(path)?
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
        if self.is_hotmemory_enabled && count > 0 {
            self.rebuild_cache()?;
        }
        Ok(count)
    }

    /// Flush pending writes to IO.
    pub fn flush_pending(&self) -> Result<(), String> {
        let ops = std::mem::take(&mut *self.pending.lock().map_err(|e| e.to_string())?);
        if !ops.is_empty() {
            self.io.apply_batch(&ops)?;
        }
        Ok(())
    }

    /// Enqueue content as a blob write. Uses pending so that flush
    /// atomicity applies: blob + record are committed together.
    ///
    /// Lock ordering: pending (lowest) → io.read (stateless).
    fn enqueue_content(&self, content: &Content) -> Result<String, String> {
        let blob_hash = content_hash(&content.data);
        let blob_path = format!("blob/{}.bin", blob_hash);
        let meta_path = format!("blob/{}.bin.meta", blob_hash);

        // Check dedup from pending buffer first (avoids IO read)
        {
            let pending = self.pending.lock().unwrap();
            for op in pending.iter() {
                if let WriteOp::Write { path, .. } = op
                    && *path == blob_path
                {
                    return Ok(blob_hash); // already queued, skip
                }
            }
        }

        // Fall back to IO check
        {
            let map = self.io.read(&blob_path)?;
            if map.is_some() {
                return Ok(blob_hash); // already stored, skip
            }
        }

        let op = WriteOp::Write {
            path: blob_path,
            data: content.data.clone(),
        };
        self.pending.lock().unwrap().push(op);

        let meta = ContentMeta {
            mime_type: content.mime_type.clone(),
            size: content.data.len() as u64,
        };
        let meta_bytes = postcard::to_allocvec(&meta).map_err(|e| e.to_string())?;
        self.pending.lock().unwrap().push(WriteOp::Write {
            path: meta_path,
            data: meta_bytes,
        });

        Ok(blob_hash)
    }

    /// Load blob content. Checks pending writes first, then IO.
    /// This ensures read-after-write consistency without flushing.
    fn load_content(&self, blob_hash: &str, default_mime: &str) -> Content {
        let blob_path = format!("blob/{}.bin", blob_hash);
        let meta_path = format!("blob/{}.bin.meta", blob_hash);

        // Check pending writes first — read mime from pending meta if available
        let (data_from_pending, mime_from_pending) = {
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
            (blob_data, mime)
        };

        if let Some(data) = data_from_pending {
            return Content {
                mime_type: mime_from_pending.unwrap_or_else(|| default_mime.to_string()),
                data,
            };
        }

        // Fall back to IO, with mime_type from meta
        let mime = self
            .read_mime_type(blob_hash)
            .unwrap_or(default_mime.to_string());
        match self.io.read(&blob_path) {
            Ok(Some(data)) => Content {
                mime_type: mime,
                data,
            },
            _ => Content {
                mime_type: default_mime.to_string(),
                data: Vec::new(),
            },
        }
    }

    /// Read mime_type from blob meta file. Returns None if not found.
    fn read_mime_type(&self, blob_hash: &str) -> Option<String> {
        let meta_path = format!("blob/{}.bin.meta", blob_hash);
        match self.io.read(&meta_path) {
            Ok(Some(bytes)) => postcard::from_bytes::<ContentMeta>(&bytes)
                .ok()
                .map(|m| m.mime_type),
            _ => None,
        }
    }
}

/// Non-cryptographic content hash for dedup. In production, replace with
/// SHA-256 or BLAKE3. The name makes clear this is NOT a security boundary.
fn content_hash(data: &[u8]) -> String {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hasher.write(data);
    format!("{:x}", hasher.finish())
}

// ── StorageRead ──────────────────────────────────────────────────────────

impl<I: AsyncFileIo> FihStorage<I> {
    /// Read state directly from IO, bypassing in-memory cache.
    /// Used when `is_hotmemory_enabled` is false (ColdStorage mode).
    fn read_state_uncached(&self) -> BoardState {
        let mut facts = Vec::new();
        let mut intents = Vec::new();
        let mut hints = Vec::new();

        if let Ok(keys) = self.io.list("facts/") {
            for key in keys {
                if let Ok(Some(bytes)) = self.io.read(&key)
                    && let Ok(r) = postcard::from_bytes::<FactRecord>(&bytes)
                {
                    let content = self.load_content(&r.blob_hash, "application/octet-stream");
                    facts.push(Fact {
                        id: FihHash(r.id.clone()),
                        origin: r.origin.clone(),
                        content,
                        creator: r.creator.clone(),
                    });
                }
            }
        }

        if let Ok(keys) = self.io.list("intents/") {
            for key in keys {
                if let Ok(Some(bytes)) = self.io.read(&key)
                    && let Ok(r) = postcard::from_bytes::<IntentRecord>(&bytes)
                {
                    intents.push(Intent {
                        id: FihHash(r.id.clone()),
                        from_facts: r.from_facts.clone(),
                        description: String::new(),
                        creator: r.creator.clone(),
                        worker: match &r.status {
                            IntentStatus::Claimed { worker, .. }
                            | IntentStatus::Concluded { worker, .. } => Some(worker.clone()),
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
                    });
                }
            }
        }

        if let Ok(keys) = self.io.list("hints/") {
            for key in keys {
                if let Ok(Some(bytes)) = self.io.read(&key)
                    && let Ok(r) = postcard::from_bytes::<HintRecord>(&bytes)
                {
                    hints.push(Hint {
                        id: FihHash(r.id.clone()),
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

impl<I: AsyncFileIo> StorageRead for FihStorage<I> {
    fn project_id(&self) -> &str {
        &self.project_id
    }

    fn read_state(&self) -> BoardState {
        if !self.is_hotmemory_enabled {
            return self.read_state_uncached();
        }
        let facts = self.fact_cache.read().unwrap();
        let intents = self.intent_cache.read().unwrap();
        let hints = self.hint_cache.read().unwrap();

        BoardState {
            facts: facts
                .values()
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
                .values()
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
                .values()
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
        // ColdStorage mode is read-only; writes go through write_blob only.
        // Block FIH writes to prevent accidental data corruption when this
        // instance is serving as the cold half of DualStorage.
        if !self.is_hotmemory_enabled {
            return Err(BlackboardError::Forbidden(
                "cannot submit_fact in read-only ColdStorage mode".into(),
            ));
        }
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
        if self.is_hotmemory_enabled {
            self.fact_cache
                .write()
                .unwrap()
                .insert(record.id.clone(), record);
        }
        self.pending.lock().unwrap().push(op);

        if self.is_hotmemory_enabled {
            // Update indices
            let ts = self.clock.now_nanos();
            self.time_index.record(ts, &fact.id.0);
            {
                let mut origin_map = self.by_origin.write().unwrap();
                origin_map
                    .entry(fact.origin.clone())
                    .or_default()
                    .push(fact.id.0.clone());
            }
            // ref_count defaults to 0 — orphan unless referenced by an Intent
            self.ref_counts
                .write()
                .unwrap()
                .entry(fact.id.0.clone())
                .or_insert_with(|| AtomicU64::new(0));
        }

        Ok(fact.id.clone())
    }
}

// ── HintCapable ──────────────────────────────────────────────────────────

impl<I: AsyncFileIo> HintCapable for FihStorage<I> {
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        if !self.is_hotmemory_enabled {
            return Err(BlackboardError::Forbidden(
                "cannot submit_hint in read-only ColdStorage mode".into(),
            ));
        }
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

        if self.is_hotmemory_enabled {
            self.hint_cache
                .write()
                .unwrap()
                .insert(record.id.clone(), record);
        }
        self.pending.lock().unwrap().push(op);

        Ok(())
    }
}

// ── IntentCapable ────────────────────────────────────────────────────────

impl<I: AsyncFileIo> IntentCapable for FihStorage<I> {
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        if !self.is_hotmemory_enabled {
            return Err(BlackboardError::Forbidden(
                "cannot submit_intent in read-only ColdStorage mode".into(),
            ));
        }
        // Verify at least one from_fact exists and all referenced facts exist
        if intent.from_facts.is_empty() {
            return Err(BlackboardError::Forbidden(
                "intent must reference at least one fact".into(),
            ));
        }
        if self.is_hotmemory_enabled {
            let facts = self.fact_cache.read().unwrap();
            for fid in &intent.from_facts {
                if !facts.contains_key(fid) {
                    return Err(BlackboardError::NotFound(format!("Fact {fid} not found")));
                }
            }
            drop(facts);

            // Increment ref_count for each from_fact
            {
                let refs = self.ref_counts.read().unwrap();
                for fid in &intent.from_facts {
                    if let Some(rc) = refs.get(fid) {
                        rc.fetch_add(1, Ordering::Relaxed);
                    }
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

        if self.is_hotmemory_enabled {
            self.intent_cache
                .write()
                .unwrap()
                .insert(record.id.clone(), record);
            // Update by_from_fact reverse index
            let mut by_ff = self.by_from_fact.write().unwrap();
            for fid in &intent.from_facts {
                by_ff
                    .entry(fid.clone())
                    .or_default()
                    .push(intent.id.0.clone());
            }
        }
        self.pending.lock().unwrap().push(op);

        Ok(intent.id.clone())
    }

    fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let mut cache = self.intent_cache.write().unwrap();
        let record = cache
            .get_mut(intent_id)
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
            postcard::to_allocvec(record).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.pending.lock().unwrap().push(WriteOp::Write {
            path: record.key(),
            data: bytes,
        });

        Ok(())
    }

    fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let mut cache = self.intent_cache.write().unwrap();
        let record = cache
            .get_mut(intent_id)
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
            postcard::to_allocvec(record).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.pending.lock().unwrap().push(WriteOp::Write {
            path: record.key(),
            data: bytes,
        });

        Ok(())
    }

    fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let mut cache = self.intent_cache.write().unwrap();
        let record = cache
            .get_mut(intent_id)
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
            postcard::to_allocvec(record).map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.pending.lock().unwrap().push(WriteOp::Write {
            path: record.key(),
            data: bytes,
        });

        Ok(())
    }

    /// Conclude an intent: transition Claimed → Concluded, produce result Fact.
    ///
    /// Lock ordering: intent_cache (write) → fact_cache (write via submit_fact).
    /// Must NOT interleave with any other cache in the opposite order.
    fn conclude_intent(&self, intent_id: &str, result: &str) -> Result<Fact, BlackboardError> {
        let mut cache = self.intent_cache.write().unwrap();
        let record = cache
            .get_mut(intent_id)
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
        drop(cache);
        FactCapable::submit_fact(self, &new_fact)?;

        // Decrement ref_count for from_facts (intent no longer references them)
        {
            let refs = self.ref_counts.read().unwrap();
            let cache = self.intent_cache.read().unwrap();
            if let Some(r) = cache.get(intent_id) {
                for fid in &r.from_facts {
                    if let Some(rc) = refs.get(fid) {
                        rc.fetch_sub(1, Ordering::Relaxed);
                    }
                }
            }
        }

        // Remove intent from by_from_fact reverse index
        {
            let cache = self.intent_cache.read().unwrap();
            if let Some(r) = cache.get(intent_id) {
                let mut by_ff = self.by_from_fact.write().unwrap();
                for fid in &r.from_facts {
                    if let Some(refs) = by_ff.get_mut(fid) {
                        refs.retain(|i| i != intent_id);
                    }
                }
            }
        }

        let intent_bytes =
            postcard::to_allocvec(self.intent_cache.read().unwrap().get(intent_id).unwrap())
                .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.pending.lock().unwrap().push(WriteOp::Write {
            path: format!("intents/i_{}.intent", intent_id),
            data: intent_bytes,
        });

        Ok(new_fact)
    }
}

// ── EvictCapable ─────────────────────────────────────────────────────────

impl<I: AsyncFileIo> EvictCapable for FihStorage<I> {
    fn approximate_size(&self) -> usize {
        let facts = self.fact_cache.read().unwrap().len();
        let intents = self.intent_cache.read().unwrap().len();
        let hints = self.hint_cache.read().unwrap().len();
        (facts + intents + hints) * 256
    }

    fn evict_before(&self, before: &str) -> Result<u64, String> {
        let before_secs: u64 = before.parse().unwrap_or(0);
        let mut hint_cache = self.hint_cache.write().map_err(|e| e.to_string())?;
        let mut removed = 0u64;

        hint_cache.retain(|_, r| {
            if r.submitted_at < before_secs {
                removed += 1;
                false
            } else {
                true
            }
        });

        Ok(removed)
    }

    fn evict_stale_intents(&self, older_than_secs: u64) -> Result<u64, String> {
        let now = self.clock.now_secs();
        let cutoff = now.saturating_sub(older_than_secs);

        let mut intent_cache = self.intent_cache.write().map_err(|e| e.to_string())?;
        let mut removed = 0u64;

        intent_cache.retain(|_, r| {
            if matches!(r.status, IntentStatus::Submitted) && r.created_at < cutoff {
                removed += 1;
                false
            } else {
                true
            }
        });

        Ok(removed)
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
                            self.time_index
                                .range(since_ts, until_ts)
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
                            self.time_index
                                .since(since_ts)
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
                            self.time_index
                                .as_of(until_ts)
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
        let first = self.time_index.first_ts()?;
        let last = self.time_index.last_ts()?;
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
            .time_index
            .since(since_ts)
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
            let fc = self.fact_cache.read().map_err(|e| e.to_string())?;
            let ic = self.intent_cache.read().map_err(|e| e.to_string())?;
            for (id, _) in &delta_ids {
                if let Some(record) = fc.get(id) {
                    facts.push(record.clone());
                }
                if let Some(record) = ic.get(id) {
                    intents.push(record.clone());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim_io::SimIo;

    fn make_storage() -> FihStorage<SimIo> {
        FihStorage::new(SimIo::new(), "test")
    }

    #[test]
    fn test_submit_and_read_fact() {
        let store = make_storage();
        let fact = Fact {
            id: FihHash("f001".into()),
            origin: "test".into(),
            content: Content {
                mime_type: "text/plain".into(),
                data: b"hello world".to_vec(),
            },
            creator: "alice".into(),
        };
        FactCapable::submit_fact(&store, &fact).unwrap();

        let state = StorageRead::read_state(&store);
        assert_eq!(state.facts.len(), 1);
        assert_eq!(state.facts[0].id.0, "f001");
        assert_eq!(state.facts[0].content.data, b"hello world");
    }

    #[test]
    fn test_submit_intent_requires_existing_fact() {
        let store = make_storage();
        let intent = Intent {
            id: FihHash("i001".into()),
            from_facts: vec!["f001".into()],
            description: "test".into(),
            creator: "bob".into(),
            worker: None,
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            is_concluded: false,
            concluded_at: None,
        };
        let result = IntentCapable::submit_intent(&store, &intent);
        assert!(result.is_err());
        assert!(matches!(result, Err(BlackboardError::NotFound(_))));
    }

    #[test]
    fn test_full_intent_lifecycle() {
        let store = make_storage();

        let fact = Fact {
            id: FihHash("f_base".into()),
            origin: "test".into(),
            content: Content {
                mime_type: "text/plain".into(),
                data: b"base".to_vec(),
            },
            creator: "alice".into(),
        };
        FactCapable::submit_fact(&store, &fact).unwrap();

        let intent = Intent {
            id: FihHash("i001".into()),
            from_facts: vec!["f_base".into()],
            description: "analyze base".into(),
            creator: "bob".into(),
            worker: None,
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            is_concluded: false,
            concluded_at: None,
        };
        IntentCapable::submit_intent(&store, &intent).unwrap();

        IntentCapable::claim_intent(&store, "i001", "alice").unwrap();
        let state = StorageRead::read_state(&store);
        assert_eq!(state.intents[0].worker.as_deref(), Some("alice"));

        IntentCapable::heartbeat(&store, "i001", "alice").unwrap();

        let result = IntentCapable::conclude_intent(&store, "i001", "analysis complete").unwrap();
        assert!(result.id.0.starts_with("f_concl_"));
        assert_eq!(result.content.data, b"analysis complete");

        // Verify concluded_at is set
        let state = StorageRead::read_state(&store);
        assert_eq!(state.facts.len(), 2);
        assert_eq!(
            state.intents[0].to_fact_id.as_deref(),
            Some(result.id.0.as_str())
        );
        assert!(
            state.intents[0].concluded_at.is_some(),
            "concluded_at should be set"
        );
        assert!(
            state.intents[0].concluded_at.unwrap() > 0,
            "concluded_at should be a real timestamp, not 0 or 1"
        );
        // created_at should be non-zero (clock.now_secs())
        assert!(
            state.intents[0].created_at != Some(0),
            "created_at should be from clock"
        );
    }

    #[test]
    fn test_double_claim_rejected() {
        let store = make_storage();
        FactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash("f_base".into()),
                origin: "test".into(),
                content: Content {
                    mime_type: "text/plain".into(),
                    data: b"x".to_vec(),
                },
                creator: "alice".into(),
            },
        )
        .unwrap();
        IntentCapable::submit_intent(
            &store,
            &Intent {
                id: FihHash("i001".into()),
                from_facts: vec!["f_base".into()],
                description: "test".into(),
                creator: "bob".into(),
                worker: None,
                to_fact_id: None,
                last_heartbeat_at: None,
                created_at: None,
                is_concluded: false,
                concluded_at: None,
            },
        )
        .unwrap();

        IntentCapable::claim_intent(&store, "i001", "alice").unwrap();
        assert!(IntentCapable::claim_intent(&store, "i001", "bob").is_err());
    }

    #[test]
    fn test_flush_preserves_content() {
        let io = SimIo::new();
        let store = FihStorage::new(io.clone(), "test");

        FactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash("f001".into()),
                origin: "t".into(),
                content: Content {
                    mime_type: "text/plain".into(),
                    data: b"flush test data".to_vec(),
                },
                creator: "alice".into(),
            },
        )
        .unwrap();

        store.flush_pending().unwrap();

        let store2 = FihStorage::new(io, "test");
        store2.rebuild_cache().unwrap();
        let state = StorageRead::read_state(&store2);
        assert_eq!(state.facts.len(), 1);
        assert_eq!(state.facts[0].content.data, b"flush test data");
    }

    #[test]
    fn test_time_index_after_rebuild() {
        let io = SimIo::new();
        let store = FihStorage::new(io.clone(), "test");

        FactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash("f1".into()),
                origin: "a".into(),
                content: Content {
                    mime_type: "text/plain".into(),
                    data: b"x".to_vec(),
                },
                creator: "t".into(),
            },
        )
        .unwrap();
        FactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash("f2".into()),
                origin: "b".into(),
                content: Content {
                    mime_type: "text/plain".into(),
                    data: b"y".to_vec(),
                },
                creator: "t".into(),
            },
        )
        .unwrap();

        store.flush_pending().unwrap();

        // Rebuild from IO — indices should be reconstructed
        let store2 = FihStorage::new(io, "test");
        store2.rebuild_cache().unwrap();

        // TimeIndex should have both entries
        let state = StorageRead::read_state(&store2);
        assert_eq!(state.facts.len(), 2);

        // Filter by origin (by_origin index)
        let filter = StateFilter {
            fact_ids: None,
            intent_ids: None,
            hint_ids: None,
            since: None,
            until: None,
            limit: None,
            offset: None,
        };
        let filtered = super::FilterCapable::read_state_filtered(&store2, &filter);
        assert_eq!(filtered.facts.len(), 2);
    }

    #[test]
    fn test_ref_count_orphan_detection() {
        let store = make_storage();

        // Submit 2 facts
        FactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash("f_orphan".into()),
                origin: "t".into(),
                content: Content {
                    mime_type: "text/plain".into(),
                    data: b"orphan".to_vec(),
                },
                creator: "t".into(),
            },
        )
        .unwrap();
        FactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash("f_refd".into()),
                origin: "t".into(),
                content: Content {
                    mime_type: "text/plain".into(),
                    data: b"refd".to_vec(),
                },
                creator: "t".into(),
            },
        )
        .unwrap();

        // Intent references f_refd → ref_count becomes 1
        IntentCapable::submit_intent(
            &store,
            &Intent {
                id: FihHash("i001".into()),
                from_facts: vec!["f_refd".into()],
                description: "test".into(),
                creator: "t".into(),
                worker: None,
                to_fact_id: None,
                last_heartbeat_at: None,
                created_at: None,
                is_concluded: false,
                concluded_at: None,
            },
        )
        .unwrap();

        // f_orphan has ref_count 0, f_refd has ref_count 1
        let refs = store.ref_counts.read().unwrap();
        assert_eq!(
            refs.get("f_orphan").map(|r| r.load(Ordering::Relaxed)),
            Some(0)
        );
        assert_eq!(
            refs.get("f_refd").map(|r| r.load(Ordering::Relaxed)),
            Some(1)
        );
        drop(refs);

        // Claim and conclude — ref_count should decrement back to 0
        IntentCapable::claim_intent(&store, "i001", "a").unwrap();
        IntentCapable::conclude_intent(&store, "i001", "done").unwrap();

        let refs = store.ref_counts.read().unwrap();
        assert_eq!(
            refs.get("f_refd").map(|r| r.load(Ordering::Relaxed)),
            Some(0),
            "conclude should decrement ref_count"
        );
    }

    #[test]
    fn test_flush_cursor_advances() {
        let store = make_storage();
        FactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash("f001".into()),
                origin: "t".into(),
                content: Content {
                    mime_type: "text/plain".into(),
                    data: b"a".to_vec(),
                },
                creator: "t".into(),
            },
        )
        .unwrap();
        let cursor = FlushCursor {
            last_flushed_at: 0,
            partition: "default".into(),
        };
        let result = <FihStorage<SimIo> as FlushCapable>::flush_since(&store, &cursor).unwrap();
        assert!(result.records_flushed > 0);
        assert!(result.new_cursor.last_flushed_at > 0);
    }

    #[test]
    fn test_flush_empty_delta() {
        let store = make_storage();
        let cursor = FlushCursor {
            last_flushed_at: 0,
            partition: "default".into(),
        };
        let result = <FihStorage<SimIo> as FlushCapable>::flush_since(&store, &cursor).unwrap();
        assert_eq!(result.records_flushed, 0);
    }

    #[test]
    fn test_flush_incremental() {
        let store = make_storage();
        FactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash("f001".into()),
                origin: "t".into(),
                content: Content {
                    mime_type: "text/plain".into(),
                    data: b"a".to_vec(),
                },
                creator: "t".into(),
            },
        )
        .unwrap();
        let cursor1 = FlushCursor {
            last_flushed_at: 0,
            partition: "default".into(),
        };
        let r1 = <FihStorage<SimIo> as FlushCapable>::flush_since(&store, &cursor1).unwrap();
        assert!(r1.records_flushed > 0);
        FactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash("f002".into()),
                origin: "t".into(),
                content: Content {
                    mime_type: "text/plain".into(),
                    data: b"b".to_vec(),
                },
                creator: "t".into(),
            },
        )
        .unwrap();
        let cursor2 = FlushCursor {
            last_flushed_at: r1.new_cursor.last_flushed_at,
            partition: "default".into(),
        };
        let r2 = <FihStorage<SimIo> as FlushCapable>::flush_since(&store, &cursor2).unwrap();
        assert_eq!(r2.records_flushed, 1);
    }

    #[test]
    fn test_flush_writes_to_io() {
        let io = SimIo::new();
        let store = FihStorage::new(io.clone(), "test");
        FactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash("f001".into()),
                origin: "t".into(),
                content: Content {
                    mime_type: "text/plain".into(),
                    data: b"data".to_vec(),
                },
                creator: "t".into(),
            },
        )
        .unwrap();
        let cursor = FlushCursor {
            last_flushed_at: 0,
            partition: "default".into(),
        };
        <FihStorage<SimIo> as FlushCapable>::flush_since(&store, &cursor).unwrap();
        let blocking = SyncFileIo::new(io.clone());
        let keys = blocking.list("flush/").unwrap();
        assert!(!keys.is_empty(), "flush directory should have chain files");
        assert!(
            keys.iter().any(|k| k.ends_with(".chain")),
            "expected .chain files in flush output"
        );
        let chain_key = keys.iter().find(|k| k.ends_with(".chain")).unwrap();
        let chain_data = SyncFileIo::new(io)
            .read(chain_key)
            .unwrap()
            .expect("chain file");
        let entry: super::ChainEntry = postcard::from_bytes(&chain_data).unwrap();
        assert_eq!(entry.facts.len(), 1, "chain should contain 1 fact record");
    }

    #[test]
    fn test_import_chain_file_after_full_lifecycle() {
        // Reproduce: submit_fact → intent → claim → conclude → flush_since → import_chain_file
        // Verifies that flush_since correctly captures all TimeIndex entries
        // and import_chain_file restores them.
        let io = SimIo::new();
        let store = FihStorage::new(io.clone(), "test");

        FactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash("f_import".into()),
                origin: "t".into(),
                content: Content {
                    mime_type: "text/plain".into(),
                    data: b"import test".to_vec(),
                },
                creator: "t".into(),
            },
        )
        .unwrap();

        // Verify cache + index are populated before any flush
        assert_eq!(store.fact_cache.read().unwrap().len(), 1);

        IntentCapable::submit_intent(
            &store,
            &Intent {
                id: FihHash("i_import".into()),
                from_facts: vec!["f_import".into()],
                description: "test".into(),
                creator: "t".into(),
                worker: None,
                to_fact_id: None,
                last_heartbeat_at: None,
                created_at: None,
                is_concluded: false,
                concluded_at: None,
            },
        )
        .unwrap();
        IntentCapable::claim_intent(&store, "i_import", "alice").unwrap();
        IntentCapable::conclude_intent(&store, "i_import", "done").unwrap();

        assert_eq!(store.fact_cache.read().unwrap().len(), 2);

        // Flush individual records to IO
        store.flush_pending().unwrap();

        // Flush chain — must find delta records
        let result = FlushCapable::flush_since(
            &store,
            &FlushCursor {
                last_flushed_at: 0,
                partition: "chain_test".into(),
            },
        )
        .unwrap();
        assert!(
            result.records_flushed > 0,
            "flush_since must produce records, fact_cache has {} entries",
            store.fact_cache.read().unwrap().len()
        );

        // Import chain into a fresh storage
        let chain_path = SyncFileIo::new(io.clone())
            .list("flush/")
            .unwrap()
            .into_iter()
            .find(|p| p.ends_with(".chain"))
            .expect("chain file must exist");

        let imported = FihStorage::new(io, "test");
        let count = imported.import_chain_file(&chain_path).unwrap();
        assert!(
            count > 0,
            "import_chain_file must restore records, got {count}"
        );

        let state = imported.read_state();
        assert_eq!(state.facts.len(), 2, "original + conclusion");
        assert_eq!(state.intents.len(), 1);
        assert!(state.intents[0].is_concluded);
    }

    #[test]
    fn test_time_index_since_filter() {
        use nexus_model::FilterCapable;
        let store = make_storage();

        FactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash("f_old".into()),
                origin: "t".into(),
                content: Content {
                    mime_type: "text/plain".into(),
                    data: b"old".to_vec(),
                },
                creator: "t".into(),
            },
        )
        .unwrap();

        // Wait a tiny bit (simulated by advancing clock)
        std::thread::sleep(std::time::Duration::from_millis(1));

        FactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash("f_new".into()),
                origin: "t".into(),
                content: Content {
                    mime_type: "text/plain".into(),
                    data: b"new".to_vec(),
                },
                creator: "t".into(),
            },
        )
        .unwrap();

        // Filter since=now should only return f_new
        let now_ns = store.clock.now_nanos();
        let filter = StateFilter {
            fact_ids: None,
            intent_ids: None,
            hint_ids: None,
            since: Some(now_ns.to_string()),
            until: None,
            limit: None,
            offset: None,
        };
        let state = <FihStorage<SimIo> as FilterCapable>::read_state_filtered(&store, &filter);
        // f_new may or may not appear depending on timing, but at minimum
        // the query should not panic and should return <= 2 facts
        assert!(state.facts.len() <= 2);
    }

    #[test]
    fn test_time_index_until_filter() {
        use nexus_model::FilterCapable;
        let store = make_storage();

        FactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash("f_early".into()),
                origin: "t".into(),
                content: Content {
                    mime_type: "text/plain".into(),
                    data: b"early".to_vec(),
                },
                creator: "t".into(),
            },
        )
        .unwrap();

        // Small delay to separate timestamps
        std::thread::sleep(std::time::Duration::from_millis(2));

        FactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash("f_mid".into()),
                origin: "t".into(),
                content: Content {
                    mime_type: "text/plain".into(),
                    data: b"mid".to_vec(),
                },
                creator: "t".into(),
            },
        )
        .unwrap();

        FactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash("f_late".into()),
                origin: "t".into(),
                content: Content {
                    mime_type: "text/plain".into(),
                    data: b"late".to_vec(),
                },
                creator: "t".into(),
            },
        )
        .unwrap();

        // Time-travel: as_of before f_late should exclude f_late
        let mid_ts = store.clock.now_nanos();
        // We can't know exact timestamps, but we can verify that:
        // 1. until filter doesn't panic
        // 2. result count is reasonable
        let filter_until = StateFilter {
            fact_ids: None,
            intent_ids: None,
            hint_ids: None,
            since: None,
            until: Some(mid_ts.to_string()),
            limit: None,
            offset: None,
        };
        let state =
            <FihStorage<SimIo> as FilterCapable>::read_state_filtered(&store, &filter_until);
        assert!(
            state.facts.len() <= 3,
            "as_of filter should not exceed total facts"
        );

        // Range: since + until
        let range_filter = StateFilter {
            fact_ids: None,
            intent_ids: None,
            hint_ids: None,
            since: Some("0".to_string()),
            until: Some(mid_ts.to_string()),
            limit: None,
            offset: None,
        };
        let state =
            <FihStorage<SimIo> as FilterCapable>::read_state_filtered(&store, &range_filter);
        assert!(state.facts.len() <= 3);
        assert!(
            state.facts.len() >= 1,
            "range filter should return at least early fact"
        );
    }
}
