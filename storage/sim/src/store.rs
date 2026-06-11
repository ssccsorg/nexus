// ── NativeFihStorage — unified FIH storage over FihIo ──────────────────
//
// Implements FactCapable, IntentCapable, HintCapable, StorageRead, and
// EvictCapable on top of a single FihIo implementation.
//
// All state transitions happen in memory first (buffer), then are flushed
// to IO via FihSession. This file handles the sync core logic.

use std::collections::HashMap;
use std::sync::{Mutex, RwLock};

use nexus_model::{
    BlackboardError, BoardState, Content, EvictCapable, Fact, FactCapable, FihHash, FilterCapable,
    Hint, HintCapable, Intent, IntentCapable, StateFilter, StorageRead,
};

use crate::io::{FihIo, FihIoBatch, WriteOp};
use crate::record::{FactRecord, HintRecord, IntentRecord, IntentStatus};

/// Unified FIH storage backended by an abstract IO layer.
///
/// All FIH trait methods are sync. They enqueue WriteOps into a buffer
/// for batch commit by the outer FihSession layer.
pub struct NativeFihStorage<I: FihIo> {
    io: I,
    project_id: String,
    // In-memory index: rebuilt from IO on hydrate, kept in sync for reads.
    // On a fresh storage with no IO reads, these caches are empty until
    // the first read_state() which scans IO.
    fact_cache: RwLock<HashMap<String, FactRecord>>,
    intent_cache: RwLock<HashMap<String, IntentRecord>>,
    hint_cache: RwLock<HashMap<String, HintRecord>>,
    // Pending writes (for FihSession coordination).
    pub(crate) pending: Mutex<Vec<WriteOp>>,
}

impl<I: FihIo> NativeFihStorage<I> {
    pub fn new(io: I, project_id: &str) -> Self {
        Self {
            io,
            project_id: project_id.to_string(),
            fact_cache: RwLock::new(HashMap::new()),
            intent_cache: RwLock::new(HashMap::new()),
            hint_cache: RwLock::new(HashMap::new()),
            pending: Mutex::new(Vec::new()),
        }
    }

    /// Rebuild in-memory cache from IO storage.
    /// Call this after hydrate to ensure read_state() is accurate.
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

    fn store_content(&self, content: &Content, io: &I) -> Result<String, String> {
        let blob_hash = sha256(&content.data);
        let blob_path = format!("blob/{}.bin", blob_hash);
        let meta_path = format!("blob/{}.bin.meta", blob_hash);

        // Only write if not already stored (content-addressable dedup)
        if io.read(&blob_path)?.is_none() {
            io.write(&blob_path, &content.data)?;
            let meta = crate::record::ContentMeta {
                mime_type: content.mime_type.clone(),
                size: content.data.len() as u64,
            };
            io.write(
                &meta_path,
                &bincode::serialize(&meta).map_err(|e| e.to_string())?,
            )?;
        }
        Ok(blob_hash)
    }
}

fn sha256(data: &[u8]) -> String {
    // Simple SHA-256 using the model types if available, or a stub for now.
    // In production this would use a proper crypto library.
    blake3_hash(data)
}

fn blake3_hash(data: &[u8]) -> String {
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
                .map(|r| Fact {
                    id: FihHash(r.id.clone()),
                    origin: r.origin.clone(),
                    content: Content {
                        mime_type: "application/octet-stream".into(),
                        data: Vec::new(), // blob content loaded separately
                    },
                    creator: r.creator.clone(),
                })
                .collect(),
            intents: intents
                .values()
                .map(|r| Intent {
                    id: FihHash(r.id.clone()),
                    from_facts: r.from_facts.clone(),
                    description: {
                        // Load description from blob if cache has it
                        // For now, return the ID as placeholder
                        r.id.clone()
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
            .store_content(&fact.content, &self.io)
            .map_err(BlackboardError::Internal)?;

        let record = FactRecord::from_model(fact, blob_hash, "0");

        let bytes =
            bincode::serialize(&record).map_err(|e| BlackboardError::Internal(e.to_string()))?;

        let path = record.key();
        let op = WriteOp::Write { path, data: bytes };

        // Update cache
        self.fact_cache
            .write()
            .unwrap()
            .insert(record.id.clone(), record.clone());
        // Queue for IO
        self.pending.lock().unwrap().push(op);

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
            submitted_at: 0,
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

        let record = IntentRecord {
            id: intent.id.0.clone(),
            from_facts: intent.from_facts.clone(),
            description_hash: String::new(), // TODO: store description as blob
            creator: intent.creator.clone(),
            status: IntentStatus::Submitted,
            created_at: 0,
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

        let new_status = record.status.try_claim(agent, 0).map_err(|e| {
            if e.starts_with("already claimed") {
                BlackboardError::Conflict(e)
            } else {
                BlackboardError::Internal(e)
            }
        })?;

        record.status = new_status;

        // Re-serialize and queue
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

        let new_status = record.status.try_heartbeat(agent, 0).map_err(|e| {
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

    fn conclude_intent(&self, intent_id: &str, result: &str) -> Result<Fact, BlackboardError> {
        let mut cache = self.intent_cache.write().unwrap();
        let record = cache
            .get_mut(intent_id)
            .ok_or_else(|| BlackboardError::NotFound(format!("Intent {intent_id} not found")))?;

        let new_status = record
            .status
            .try_conclude("", 0)
            .map_err(BlackboardError::Internal)?;

        // Extract worker before moving
        let worker = match &new_status {
            IntentStatus::Concluded { worker, .. } => worker.clone(),
            _ => unreachable!(),
        };

        record.status = new_status;

        // Create conclusion Fact
        let new_fact = Fact {
            id: FihHash(format!("f_concl_{}", intent_id)),
            origin: format!("conclusion:{}", intent_id),
            content: Content {
                mime_type: "text/plain".into(),
                data: result.as_bytes().to_vec(),
            },
            creator: worker,
        };

        // Update conclusion fact ID in status
        let conclusion_fact_id = new_fact.id.0.clone();
        if let IntentStatus::Concluded {
            ref mut to_fact, ..
        } = record.status
        {
            to_fact.clone_from(&conclusion_fact_id);
        }

        // Submit conclusion fact via FactCapable
        drop(cache); // release cache lock before re-entering
        self.submit_fact(&new_fact)?;

        // Re-serialize intent
        let bytes = bincode::serialize(self.intent_cache.read().unwrap().get(intent_id).unwrap())
            .map_err(|e| BlackboardError::Internal(e.to_string()))?;
        self.pending.lock().unwrap().push(WriteOp::Write {
            path: format!("intents/i_{}.intent", intent_id),
            data: bytes,
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
        (facts + intents + hints) * 256 // rough estimate
    }

    fn evict_before(&self, before: &str) -> Result<u64, String> {
        // Simplified: remove hints by TTL, remove concluded intents older than before
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
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
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
        let mut state = self.read_state();

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
        store.submit_fact(&fact).unwrap();

        let state = store.read_state();
        assert_eq!(state.facts.len(), 1);
        assert_eq!(state.facts[0].id.0, "f001");
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
        let result = store.submit_intent(&intent);
        assert!(result.is_err());
        assert!(matches!(result, Err(BlackboardError::NotFound(_))));
    }

    #[test]
    fn test_full_intent_lifecycle() {
        let store = make_storage();

        // Submit fact
        let fact = Fact {
            id: FihHash("f_base".into()),
            origin: "test".into(),
            content: Content {
                mime_type: "text/plain".into(),
                data: b"base".to_vec(),
            },
            creator: "alice".into(),
        };
        store.submit_fact(&fact).unwrap();

        // Submit intent
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
        store.submit_intent(&intent).unwrap();

        // Claim
        store.claim_intent("i001", "alice").unwrap();
        let state = store.read_state();
        assert_eq!(state.intents[0].worker.as_deref(), Some("alice"));

        // Heartbeat
        store.heartbeat("i001", "alice").unwrap();

        // Conclude
        let result = store.conclude_intent("i001", "analysis complete").unwrap();
        assert!(result.id.0.starts_with("f_concl_"));

        // Verify conclusion fact exists
        let state = store.read_state();
        assert_eq!(state.facts.len(), 2);
        assert_eq!(
            state.intents[0].to_fact_id.as_deref(),
            Some(result.id.0.as_str())
        );
    }

    #[test]
    fn test_double_claim_rejected() {
        let store = make_storage();
        store
            .submit_fact(&Fact {
                id: FihHash("f_base".into()),
                origin: "test".into(),
                content: Content {
                    mime_type: "text/plain".into(),
                    data: b"x".to_vec(),
                },
                creator: "alice".into(),
            })
            .unwrap();
        store
            .submit_intent(&Intent {
                id: FihHash("i001".into()),
                from_facts: vec!["f_base".into()],
                description: "test".into(),
                creator: "bob".into(),
                worker: None,
                to_fact_id: None,
                last_heartbeat_at: None,
                created_at: None,
                concluded_at: None,
            })
            .unwrap();

        store.claim_intent("i001", "alice").unwrap();
        assert!(store.claim_intent("i001", "bob").is_err());
    }
}
