// ── NativeFihStorage — unified FIH storage over FihIo ──────────────────
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
    Hint, HintCapable, Intent, IntentCapable, Now, StateFilter, StorageRead,
};

use crate::index::TimeIndex;
use crate::io::{FihIo, FihIoBatch, WriteOp};
use crate::record::{ContentMeta, FactRecord, HintRecord, IntentRecord, IntentStatus};

/// Unified FIH storage backended by an abstract IO layer.
///
/// All FIH trait methods are sync. They enqueue WriteOps into a buffer
/// for batch commit by the outer FihSession layer.
pub struct NativeFihStorage<I: FihIo> {
    io: I,
    project_id: String,
    clock: Box<dyn Now + Send>,
    // In-memory cache: rebuilt from IO on hydrate, kept in sync for reads.
    fact_cache: RwLock<HashMap<String, FactRecord>>,
    intent_cache: RwLock<HashMap<String, IntentRecord>>,
    hint_cache: RwLock<HashMap<String, HintRecord>>,
    // Indices
    time_index: TimeIndex,
    ref_counts: RwLock<HashMap<String, AtomicU64>>,
    by_origin: RwLock<HashMap<String, Vec<String>>>,
    // Pending writes (for FihSession coordination).
    pub(crate) pending: Mutex<Vec<WriteOp>>,
}

impl<I: FihIo> NativeFihStorage<I> {
    pub fn new(io: I, project_id: &str) -> Self {
        Self::with_clock(io, project_id, Box::new(nexus_model::SystemClock))
    }

    pub fn with_clock(io: I, project_id: &str, clock: Box<dyn Now + Send>) -> Self {
        Self {
            io,
            project_id: project_id.to_string(),
            clock,
            fact_cache: RwLock::new(HashMap::new()),
            intent_cache: RwLock::new(HashMap::new()),
            hint_cache: RwLock::new(HashMap::new()),
            time_index: TimeIndex::new(),
            ref_counts: RwLock::new(HashMap::new()),
            by_origin: RwLock::new(HashMap::new()),
            pending: Mutex::new(Vec::new()),
        }
    }

    /// Rebuild in-memory cache from IO storage.
    pub fn rebuild_cache(&self) -> Result<(), String> {
        let fact_keys = self.io.list("facts/")?;
        let mut facts = HashMap::new();
        for key in fact_keys {
            if let Some(bytes) = self.io.read(&key)?
                && let Ok(record) = bincode::deserialize::<FactRecord>(&bytes)
            {
                facts.insert(record.id.clone(), record);
            }
        }

        let intent_keys = self.io.list("intents/")?;
        let mut intents = HashMap::new();
        for key in intent_keys {
            if let Some(bytes) = self.io.read(&key)?
                && let Ok(record) = bincode::deserialize::<IntentRecord>(&bytes)
            {
                intents.insert(record.id.clone(), record);
            }
        }

        let hint_keys = self.io.list("hints/")?;
        let mut hints = HashMap::new();
        for key in hint_keys {
            if let Some(bytes) = self.io.read(&key)?
                && let Ok(record) = bincode::deserialize::<HintRecord>(&bytes)
            {
                hints.insert(record.id.clone(), record);
            }
        }

        *self.fact_cache.write().map_err(|e| e.to_string())? = facts;
        *self.intent_cache.write().map_err(|e| e.to_string())? = intents;
        *self.hint_cache.write().map_err(|e| e.to_string())? = hints;
        Ok(())
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
        let meta_bytes = bincode::serialize(&meta).map_err(|e| e.to_string())?;
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
                        if let Ok(meta) = bincode::deserialize::<ContentMeta>(data) {
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
            Ok(Some(bytes)) => bincode::deserialize::<ContentMeta>(&bytes)
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

impl<I: FihIo> StorageRead for NativeFihStorage<I> {
    fn project_id(&self) -> &str {
        &self.project_id
    }

    fn read_state(&self) -> BoardState {
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
                        } => Some(last_heartbeat_at.to_string()),
                        _ => None,
                    },
                    created_at: Some(r.created_at.to_string()),
                    concluded_at: match &r.status {
                        IntentStatus::Concluded { .. } => Some("yes".into()),
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

impl<I: FihIo> FactCapable for NativeFihStorage<I> {
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        let blob_hash = self
            .enqueue_content(&fact.content)
            .map_err(BlackboardError::Internal)?;

        let record = FactRecord::from_model(fact, blob_hash, &self.clock.now_nanos());

        let bytes =
            bincode::serialize(&record).map_err(|e| BlackboardError::Internal(e.to_string()))?;

        let op = WriteOp::Write {
            path: record.key(),
            data: bytes,
        };

        // Update cache immediately for subsequent reads
        self.fact_cache
            .write()
            .unwrap()
            .insert(record.id.clone(), record);
        self.pending.lock().unwrap().push(op);

        // Update indices
        let ts = self.clock.now_nanos().parse::<u64>().unwrap_or(0);
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

        Ok(fact.id.clone())
    }
}

// ── HintCapable ──────────────────────────────────────────────────────────

impl<I: FihIo> HintCapable for NativeFihStorage<I> {
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        let record = HintRecord {
            id: hint.id.0.clone(),
            content: hint.content.clone(),
            creator: hint.creator.clone(),
            submitted_at: self.clock.now_secs(),
            ttl_secs: None,
        };

        let bytes =
            bincode::serialize(&record).map_err(|e| BlackboardError::Internal(e.to_string()))?;

        let op = WriteOp::Write {
            path: record.key(),
            data: bytes,
        };

        self.hint_cache
            .write()
            .unwrap()
            .insert(record.id.clone(), record);
        self.pending.lock().unwrap().push(op);

        Ok(())
    }
}

// ── IntentCapable ────────────────────────────────────────────────────────

impl<I: FihIo> IntentCapable for NativeFihStorage<I> {
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        // Verify all from_facts exist in cache
        let facts = self.fact_cache.read().unwrap();
        for fid in &intent.from_facts {
            if !facts.contains_key(fid) {
                return Err(BlackboardError::NotFound(format!("Fact {fid} not found")));
            }
        }
        drop(facts);

        // Increment ref_count for each from_fact
        {
            let refs = self.ref_counts.write().unwrap();
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
            bincode::serialize(&record).map_err(|e| BlackboardError::Internal(e.to_string()))?;

        let op = WriteOp::Write {
            path: record.key(),
            data: bytes,
        };

        self.intent_cache
            .write()
            .unwrap()
            .insert(record.id.clone(), record);
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
            bincode::serialize(record).map_err(|e| BlackboardError::Internal(e.to_string()))?;
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
            bincode::serialize(record).map_err(|e| BlackboardError::Internal(e.to_string()))?;
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
            bincode::serialize(record).map_err(|e| BlackboardError::Internal(e.to_string()))?;
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
        let now_ns = self.clock.now_nanos().parse::<u64>().unwrap_or(0);
        let new_status = record
            .status
            .try_conclude(&conclusion_fact_id, now_ns)
            .map_err(BlackboardError::Internal)?;

        record.status = new_status;

        // Submit conclusion fact via FactCapable, then re-serialize intent
        drop(cache);
        FactCapable::submit_fact(self, &new_fact)?;

        let intent_bytes =
            bincode::serialize(self.intent_cache.read().unwrap().get(intent_id).unwrap())
                .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.pending.lock().unwrap().push(WriteOp::Write {
            path: format!("intents/i_{}.intent", intent_id),
            data: intent_bytes,
        });

        Ok(new_fact)
    }
}

// ── EvictCapable ─────────────────────────────────────────────────────────

impl<I: FihIo> EvictCapable for NativeFihStorage<I> {
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

impl<I: FihIo> FilterCapable for NativeFihStorage<I> {
    fn read_state_filtered(&self, filter: &StateFilter) -> BoardState {
        let mut state = StorageRead::read_state(self);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim_io::SimFihIo;

    fn make_storage() -> NativeFihStorage<SimFihIo> {
        NativeFihStorage::new(SimFihIo::new(), "test")
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
        // created_at should be non-zero (clock.now_secs())
        assert!(
            state.intents[0].created_at.as_deref() != Some("0"),
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
                concluded_at: None,
            },
        )
        .unwrap();

        IntentCapable::claim_intent(&store, "i001", "alice").unwrap();
        assert!(IntentCapable::claim_intent(&store, "i001", "bob").is_err());
    }

    #[test]
    fn test_flush_preserves_content() {
        let io = SimFihIo::new();
        let store = NativeFihStorage::new(io.clone(), "test");

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

        let store2 = NativeFihStorage::new(io, "test");
        store2.rebuild_cache().unwrap();
        let state = StorageRead::read_state(&store2);
        assert_eq!(state.facts.len(), 1);
        assert_eq!(state.facts[0].content.data, b"flush test data");
    }
}
