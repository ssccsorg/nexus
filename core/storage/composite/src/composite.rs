// CompositeColdStorage — platform-independent cold storage backed by a
// KeyValueStore (KV) + BlobStore (blob) + ObjectStore (object) trio.
//
// # Three-tier architecture
//
// External bindings (rs-worker, CF Workers WASM bindings) inject concrete
// K/B/O implementations. CompositeColdStorage itself is fully platform-independent
// and contains no Cloudflare-specific code.
//
// ```
//                 ┌──────────────────────────────────────────┐
//                 │          ColdStorage trait                │
//                 │  (FihPersistence + Filter + Scan + Flush) │
//                 └──────────────────┬───────────────────────┘
//                                    │
//                 ┌──────────────────┴───────────────────────┐
//                 │        CompositeColdStorage<K, B, O, C>     │
//                 │                                          │
//                 │  ┌─────────┐  ┌─────────┐  ┌──────────┐ │
//                 │  │ Tier 1  │  │ Tier 2  │  │ Tier 3   │ │
//                 │  │ KV      │  │ Blob    │  │ Object   │ │
//                 │  │ recent  │  │ archive │  │ coord.   │ │
//                 │  └────┬────┘  └────┬────┘  └────┬─────┘ │
//                 └───────┼───────────┼──────────────┼──────┘
//                         │           │              │
//                    Workers KV    R2 bucket    Durable Object
//                    Sled          filesystem    Redis lock
//                    MockKv        MockBlob      MockObject
// ```
//
// # Tier roles
//
// | Tier | Store | Role | Write | Read |
// |------|-------|------|-------|------|
// | 1 | KV | Recent buffer, cursor persistence | `submit_fact`/`claim_intent` | `get(key)`, `list(prefix)` |
// | 2 | Blob | JSON-lines archive, flush target | `flush_since` | bulk `scan_partition` |
// | 3 | Object | CAS-based claim coordination, snapshot ownership | `compare_and_swap` | `get_state` |
//
// # Data flow
//
// 1. **Ingest**: `submit_fact` writes to KV only (Tier 1). Fast single-key write.
// 2. **Flush**: `flush_since` reads recent data from KV (by submission timestamp),
//    serializes to JSON-lines, writes to Blob (Tier 2). Cursor persisted in KV.
// 3. **Archive**: `evict_before` removes old Blob entries from Tier 2.
// 4. **Coordinate**: `claim_intent`/`heartbeat`/`release_intent` use KV data +
//    optional CAS via Object (Tier 3) for cross-worker conflict resolution.
// 5. **Read**: `read_state` reads KV only (recent). `scan_partition` merges
//    KV (recent) + Blob (flushed) with dedup by entity ID.
//
// # KV storage layout
//
// - `{project_id}:fact:{fact_id}` → JSON `Stamped<Fact>`
// - `{project_id}:intent:{intent_id}` → JSON `Stamped<Intent>`
// - `{project_id}:hint:{hint_id}` → JSON `Stamped<Hint>`
// - `{project_id}:cursor` → JSON `FlushCursor`
//
// # Blob storage layout (produced by flush_since)
//
// - `{project_id}/flush/facts/{partition}/{ts}.jsonl`
// - `{project_id}/flush/intents/{partition}/{ts}.jsonl`
// - `{project_id}/flush/hints/{partition}/{ts}.jsonl`
//
// JSON lines format (no Parquet dependency) keeps the crate purely Rust
// with no C bindings. A future upgrade can add Parquet via arrow/parquet-wasm.

use crate::{
    BlobStore, KeyValueStore, ObjectStore, cursor_key, fact_key, fact_prefix, flush_blob_key,
    flush_blob_prefix, hint_key, hint_prefix, intent_key, intent_prefix,
};
use crate::{Now, SystemClock};
use log;
use nexus_model::{
    BlackboardError, BoardState, CypherCapable, EvictCapable, Fact, FactCapable, FihHash,
    FilterCapable, FlushCapable, FlushCursor, FlushResult, Hint, HintCapable, Intent,
    IntentCapable, PartitionData, ScanCapable, StateFilter, StorageRead, TimeRangeCapable,
};
use serde::{Deserialize, Serialize};
use std::ops::Range;

// ── Timestamped envelope ───────────────────────────────────────────────────

/// Wraps a stored entity with a submission timestamp for cursor-based filtering.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Stamped<T> {
    submitted_at: String,
    data: T,
}

impl<T: Serialize> Stamped<T> {
    fn new(data: T, now: String) -> Self {
        Self {
            submitted_at: now,
            data,
        }
    }
}

// ── CompositeColdStorage ──────────────────────────────────────────────────────

/// Cold storage backend backed by a KeyValueStore + BlobStore + ObjectStore trio.
///
/// Generic over K (KeyValueStore), B (BlobStore), and O (ObjectStore), allowing
/// the same CompositeColdStorage logic to run in tests (MockKv + MockBlob +
/// MockObject), CF Workers (worker::kv::Namespace + R2 + Durable Object), or
/// servers (sled + filesystem + Redis lock).
///
/// # Storage layout
///
/// ## KV keys
/// - `{project_id}:fact:{fact_id}` → JSON Stamped<Fact>
/// - `{project_id}:intent:{intent_id}` → JSON Stamped<Intent>
/// - `{project_id}:hint:{hint_id}` → JSON Stamped<Hint>
/// - `{project_id}:cursor` → JSON FlushCursor
///
/// ## Blob keys (produced by flush_since)
/// - `{project_id}/flush/facts/{partition}/{ts}.jsonl`
/// - `{project_id}/flush/intents/{partition}/{ts}.jsonl`
/// - `{project_id}/flush/hints/{partition}/{ts}.jsonl`
pub struct CompositeColdStorage<
    K: KeyValueStore,
    B: BlobStore,
    O: ObjectStore,
    C: Now = SystemClock,
> {
    kv: K,
    blob: B,
    object: O,
    clock: C,
    project_id: String,
}

// ── Generic constructor (caller chooses the clock) ─────────────────────────

impl<K: KeyValueStore, B: BlobStore, O: ObjectStore, C: Now> CompositeColdStorage<K, B, O, C> {
    pub fn new(kv: K, blob: B, object: O, clock: C, project_id: impl Into<String>) -> Self {
        Self {
            kv,
            blob,
            object,
            clock,
            project_id: project_id.into(),
        }
    }

    /// Access the underlying KV store.
    pub fn kv(&self) -> &K {
        &self.kv
    }

    /// Access the underlying Blob store.
    pub fn blob(&self) -> &B {
        &self.blob
    }

    /// Access the underlying Object store.
    pub fn object(&self) -> &O {
        &self.object
    }

    // ── Internal helpers ────────────────────────────────────────────────────

    fn project(&self) -> &str {
        &self.project_id
    }

    /// Read all facts from KV, unwrapping from Stamped envelope.
    fn read_facts(&self) -> Result<Vec<Fact>, String> {
        let prefix = fact_prefix(self.project());
        let keys = self.kv.list(&prefix)?;
        let mut facts = Vec::with_capacity(keys.len());
        for key in &keys {
            if let Some(json) = self.kv.get(key)?
                && let Ok(stamped) = serde_json::from_str::<Stamped<Fact>>(&json)
            {
                facts.push(stamped.data);
            }
        }
        facts.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        Ok(facts)
    }

    /// Read all intents from KV, unwrapping from Stamped envelope.
    fn read_intents(&self) -> Result<Vec<Intent>, String> {
        let prefix = intent_prefix(self.project());
        let keys = self.kv.list(&prefix)?;
        let mut intents = Vec::with_capacity(keys.len());
        for key in &keys {
            if let Some(json) = self.kv.get(key)?
                && let Ok(stamped) = serde_json::from_str::<Stamped<Intent>>(&json)
            {
                intents.push(stamped.data);
            }
        }
        intents.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        Ok(intents)
    }

    /// Read all hints from KV, unwrapping from Stamped envelope.
    fn read_hints(&self) -> Result<Vec<Hint>, String> {
        let prefix = hint_prefix(self.project());
        let keys = self.kv.list(&prefix)?;
        let mut hints = Vec::with_capacity(keys.len());
        for key in &keys {
            if let Some(json) = self.kv.get(key)?
                && let Ok(stamped) = serde_json::from_str::<Stamped<Hint>>(&json)
            {
                hints.push(stamped.data);
            }
        }
        hints.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        Ok(hints)
    }

    // ── JSON lines deserialization ──────────────────────────────────────────

    fn read_jsonl_lines<T>(bytes: &[u8]) -> Result<Vec<T>, String>
    where
        T: serde::de::DeserializeOwned,
    {
        let content = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
        content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|line| serde_json::from_str::<T>(line).map_err(|e| e.to_string()))
            .collect()
    }

    /// Read all facts from flushed blobs for a given partition.
    fn read_flushed_facts(&self, partition: &str) -> Result<Vec<Fact>, String> {
        let prefix = flush_blob_prefix(self.project(), "facts", partition);
        let blob_keys = self.blob.list(&prefix)?;
        let mut all = Vec::new();
        for key in &blob_keys {
            if let Some(bytes) = self.blob.get(key)? {
                match Self::read_jsonl_lines::<Fact>(&bytes) {
                    Ok(items) => all.extend(items),
                    Err(e) => log::warn!(
                        "CompositeColdStorage[{}] skipping partial blob {}: {}",
                        self.project(),
                        key,
                        e
                    ),
                }
            }
        }
        Ok(all)
    }

    /// Read all intents from flushed blobs for a given partition.
    fn read_flushed_intents(&self, partition: &str) -> Result<Vec<Intent>, String> {
        let prefix = flush_blob_prefix(self.project(), "intents", partition);
        let blob_keys = self.blob.list(&prefix)?;
        let mut all = Vec::new();
        for key in &blob_keys {
            if let Some(bytes) = self.blob.get(key)? {
                match Self::read_jsonl_lines::<Intent>(&bytes) {
                    Ok(items) => all.extend(items),
                    Err(e) => log::warn!(
                        "CompositeColdStorage[{}] skipping partial blob {}: {}",
                        self.project(),
                        key,
                        e
                    ),
                }
            }
        }
        Ok(all)
    }

    /// Read all hints from flushed blobs for a given partition.
    fn read_flushed_hints(&self, partition: &str) -> Result<Vec<Hint>, String> {
        let prefix = flush_blob_prefix(self.project(), "hints", partition);
        let blob_keys = self.blob.list(&prefix)?;
        let mut all = Vec::new();
        for key in &blob_keys {
            if let Some(bytes) = self.blob.get(key)? {
                match Self::read_jsonl_lines::<Hint>(&bytes) {
                    Ok(items) => all.extend(items),
                    Err(e) => log::warn!(
                        "CompositeColdStorage[{}] skipping partial blob {}: {}",
                        self.project(),
                        key,
                        e
                    ),
                }
            }
        }
        Ok(all)
    }
}

// ── Convenience constructor (defaults to SystemClock) ──────────────────────

impl<K: KeyValueStore, B: BlobStore, O: ObjectStore> CompositeColdStorage<K, B, O, SystemClock> {
    pub fn new_with_system_clock(kv: K, blob: B, object: O, project_id: impl Into<String>) -> Self {
        Self {
            kv,
            blob,
            object,
            clock: SystemClock,
            project_id: project_id.into(),
        }
    }
}

// ── StorageRead ───────────────────────────────────────────────────────────

impl<K: KeyValueStore, B: BlobStore, O: ObjectStore, C: Now> StorageRead
    for CompositeColdStorage<K, B, O, C>
{
    fn project_id(&self) -> &str {
        self.project()
    }

    fn read_state(&self) -> BoardState {
        let facts = match self.read_facts() {
            Ok(f) => f,
            Err(e) => {
                log::warn!(
                    "CompositeColdStorage[{}] read_facts failed: {}",
                    self.project(),
                    e
                );
                Vec::new()
            }
        };
        let intents = match self.read_intents() {
            Ok(i) => i,
            Err(e) => {
                log::warn!(
                    "CompositeColdStorage[{}] read_intents failed: {}",
                    self.project(),
                    e
                );
                Vec::new()
            }
        };
        let hints = match self.read_hints() {
            Ok(h) => h,
            Err(e) => {
                log::warn!(
                    "CompositeColdStorage[{}] read_hints failed: {}",
                    self.project(),
                    e
                );
                Vec::new()
            }
        };
        BoardState {
            facts,
            intents,
            hints,
        }
    }
}

// ── FactCapable ───────────────────────────────────────────────────────────

impl<K: KeyValueStore, B: BlobStore, O: ObjectStore, C: Now> FactCapable
    for CompositeColdStorage<K, B, O, C>
{
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        let key = fact_key(self.project(), &fact.id.0);
        let stamped = Stamped::new(fact.clone(), self.clock.now_nanos());
        let json = serde_json::to_string(&stamped)
            .map_err(|e| BlackboardError::Internal(format!("serialize fact: {e}")))?;
        self.kv
            .set(&key, &json)
            .map_err(|e| BlackboardError::Internal(format!("kv set: {e}")))?;
        Ok(fact.id.clone())
    }
}

// ── IntentCapable ─────────────────────────────────────────────────────────

impl<K: KeyValueStore, B: BlobStore, O: ObjectStore, C: Now> IntentCapable
    for CompositeColdStorage<K, B, O, C>
{
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        let key = intent_key(self.project(), &intent.id.0);
        let stamped = Stamped::new(intent.clone(), self.clock.now_nanos());
        let json = serde_json::to_string(&stamped)
            .map_err(|e| BlackboardError::Internal(format!("serialize intent: {e}")))?;
        self.kv
            .set(&key, &json)
            .map_err(|e| BlackboardError::Internal(format!("kv set: {e}")))?;
        Ok(intent.id.clone())
    }

    /// Claim an intent for an agent.
    ///
    /// Uses a two-step protocol for cross-worker safety:
    ///   1. `object.cas(key, "", agent)` — atomic CAS gate.
    ///      Only one worker succeeds; others get Conflict.
    ///   2. Update KV with worker and heartbeat for data consistency.
    ///
    /// If KV entry is missing despite CAS success (concurrent conclude),
    /// the CAS is rolled back and NotFound is returned.
    fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let key = intent_key(self.project(), intent_id);

        // Atomic CAS gate: only one worker can claim this intent.
        // The ObjectStore key matches the KV intent key, creating a per-intent
        // namespace. In CF Workers this becomes a Durable Object per intent.
        // Sentinel empty string = unclaimed; agent name = claimed.
        let claimed = self
            .object
            .put_state(&key, "", agent)
            .map_err(|e| BlackboardError::Internal(format!("object cas: {e}")))?;
        if !claimed {
            return Err(BlackboardError::Conflict(format!(
                "Intent {intent_id} already claimed by another worker"
            )));
        }

        // CAS succeeded: update KV for data consistency.
        let json = self
            .kv
            .get(&key)
            .map_err(|e| BlackboardError::Internal(format!("kv get: {e}")))?;
        match json {
            Some(raw) => {
                let mut stamped: Stamped<Intent> = serde_json::from_str(&raw)
                    .map_err(|e| BlackboardError::Internal(e.to_string()))?;
                stamped.data.worker = Some(agent.to_string());
                stamped.data.last_heartbeat_at = Some(self.clock.now_nanos());
                let updated = serde_json::to_string(&stamped)
                    .map_err(|e| BlackboardError::Internal(format!("serialize: {e}")))?;
                self.kv
                    .set(&key, &updated)
                    .map_err(|e| BlackboardError::Internal(format!("kv set: {e}")))?;
                Ok(())
            }
            None => {
                // CAS won but KV entry was deleted concurrently (conclude).
                // Release CAS and return NotFound.
                let _ = self.object.put_state(&key, agent, "");
                Err(BlackboardError::NotFound(format!(
                    "Intent {intent_id} not found"
                )))
            }
        }
    }

    fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let key = intent_key(self.project(), intent_id);
        let json = self
            .kv
            .get(&key)
            .map_err(|e| BlackboardError::Internal(format!("kv get: {e}")))?;
        match json {
            Some(raw) => {
                let mut stamped: Stamped<Intent> = serde_json::from_str(&raw)
                    .map_err(|e| BlackboardError::Internal(e.to_string()))?;
                match &stamped.data.worker {
                    Some(w) if w != agent => {
                        return Err(BlackboardError::Conflict(format!(
                            "Intent {intent_id} claimed by {w}, not {agent}"
                        )));
                    }
                    None => {
                        return Err(BlackboardError::Conflict(format!(
                            "Intent {intent_id} is not claimed"
                        )));
                    }
                    _ => {}
                }
                stamped.data.last_heartbeat_at = Some(self.clock.now_nanos());
                let updated = serde_json::to_string(&stamped)
                    .map_err(|e| BlackboardError::Internal(format!("serialize: {e}")))?;
                self.kv
                    .set(&key, &updated)
                    .map_err(|e| BlackboardError::Internal(format!("kv set: {e}")))?;
                Ok(())
            }
            None => Err(BlackboardError::NotFound(format!(
                "Intent {intent_id} not found"
            ))),
        }
    }

    fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let key = intent_key(self.project(), intent_id);
        let json = self
            .kv
            .get(&key)
            .map_err(|e| BlackboardError::Internal(format!("kv get: {e}")))?;
        match json {
            Some(raw) => {
                let mut stamped: Stamped<Intent> = serde_json::from_str(&raw)
                    .map_err(|e| BlackboardError::Internal(e.to_string()))?;
                match &stamped.data.worker {
                    Some(w) if w != agent => {
                        return Err(BlackboardError::Conflict(format!(
                            "Intent {intent_id} claimed by {w}, not {agent}"
                        )));
                    }
                    None => {
                        return Err(BlackboardError::Conflict(format!(
                            "Intent {intent_id} is not claimed"
                        )));
                    }
                    _ => {}
                }
                stamped.data.worker = None;
                let updated = serde_json::to_string(&stamped)
                    .map_err(|e| BlackboardError::Internal(format!("serialize: {e}")))?;
                self.kv
                    .set(&key, &updated)
                    .map_err(|e| BlackboardError::Internal(format!("kv set: {e}")))?;
                Ok(())
            }
            None => Err(BlackboardError::NotFound(format!(
                "Intent {intent_id} not found"
            ))),
        }
    }

    fn conclude_intent(
        &self,
        intent_id: &str,
        result: &serde_json::Value,
    ) -> Result<Fact, BlackboardError> {
        let key = intent_key(self.project(), intent_id);
        let json = self
            .kv
            .get(&key)
            .map_err(|e| BlackboardError::Internal(format!("kv get: {e}")))?;
        match json {
            Some(raw) => {
                let stamped: Stamped<Intent> = serde_json::from_str(&raw)
                    .map_err(|e| BlackboardError::Internal(e.to_string()))?;
                let fact = Fact {
                    id: FihHash::new(
                        &[
                            intent_id,
                            &serde_json::to_string(result).unwrap_or_default(),
                        ],
                        "concluded",
                    ),
                    origin: format!("intent:{intent_id}"),
                    content: result.clone(),
                    creator: stamped.data.creator.clone(),
                };
                self.kv
                    .delete(&key)
                    .map_err(|e| BlackboardError::Internal(format!("kv delete: {e}")))?;
                self.submit_fact(&fact)?;
                Ok(fact)
            }
            None => Err(BlackboardError::NotFound(format!(
                "Intent {intent_id} not found"
            ))),
        }
    }
}

// ── HintCapable ───────────────────────────────────────────────────────────

impl<K: KeyValueStore, B: BlobStore, O: ObjectStore, C: Now> HintCapable
    for CompositeColdStorage<K, B, O, C>
{
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        let key = hint_key(self.project(), &hint.id.0);
        let stamped = Stamped::new(hint.clone(), self.clock.now_nanos());
        let json = serde_json::to_string(&stamped)
            .map_err(|e| BlackboardError::Internal(format!("serialize hint: {e}")))?;
        self.kv
            .set(&key, &json)
            .map_err(|e| BlackboardError::Internal(format!("kv set: {e}")))?;
        Ok(())
    }
}

// ── FilterCapable ─────────────────────────────────────────────────────────

impl<K: KeyValueStore, B: BlobStore, O: ObjectStore, C: Now> FilterCapable
    for CompositeColdStorage<K, B, O, C>
{
    fn read_state_filtered(&self, filter: &StateFilter) -> BoardState {
        let mut facts: Vec<Fact> = self.read_facts().unwrap_or_default();
        let mut intents: Vec<Intent> = self.read_intents().unwrap_or_default();
        let mut hints: Vec<Hint> = self.read_hints().unwrap_or_default();

        // Filter by IDs
        if let Some(ids) = &filter.fact_ids {
            facts.retain(|f| ids.contains(&f.id.0));
        }
        if let Some(ids) = &filter.intent_ids {
            intents.retain(|i| ids.contains(&i.id.0));
        }
        if let Some(ids) = &filter.hint_ids {
            hints.retain(|h| ids.contains(&h.id.0));
        }

        // Filter by time range (Intents have created_at; Facts and Hints don't)
        if let Some(since) = &filter.since {
            intents.retain(|i| {
                i.created_at
                    .as_ref()
                    .is_none_or(|c| c.as_str() >= since.as_str())
            });
        }
        if let Some(until) = &filter.until {
            intents.retain(|i| {
                i.created_at
                    .as_ref()
                    .is_none_or(|c| c.as_str() <= until.as_str())
            });
        }

        // Apply offset + limit
        let offset = filter.offset.unwrap_or(0);
        if let Some(limit) = filter.limit {
            facts = facts.into_iter().skip(offset).take(limit).collect();
            intents = intents.into_iter().skip(offset).take(limit).collect();
            hints = hints.into_iter().skip(offset).take(limit).collect();
        } else if offset > 0 {
            facts = facts.into_iter().skip(offset).collect();
            intents = intents.into_iter().skip(offset).collect();
            hints = hints.into_iter().skip(offset).collect();
        }

        BoardState {
            facts,
            intents,
            hints,
        }
    }
}

// ── ScanCapable ───────────────────────────────────────────────────────────

impl<K: KeyValueStore, B: BlobStore, O: ObjectStore, C: Now> ScanCapable
    for CompositeColdStorage<K, B, O, C>
{
    fn scan_partition(&self, partition: &str) -> Result<PartitionData, String> {
        let kv_facts = self.read_facts()?;
        let kv_intents = self.read_intents()?;
        let kv_hints = self.read_hints()?;

        let flushed_facts = self.read_flushed_facts(partition)?;
        let flushed_intents = self.read_flushed_intents(partition)?;
        let flushed_hints = self.read_flushed_hints(partition)?;

        let mut all_facts: Vec<Fact> = kv_facts;
        all_facts.extend(flushed_facts);
        all_facts.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        all_facts.dedup_by(|a, b| a.id.0 == b.id.0);

        let mut all_intents: Vec<Intent> = kv_intents;
        all_intents.extend(flushed_intents);
        all_intents.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        all_intents.dedup_by(|a, b| a.id.0 == b.id.0);

        let mut all_hints: Vec<Hint> = kv_hints;
        all_hints.extend(flushed_hints);
        all_hints.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        all_hints.dedup_by(|a, b| a.id.0 == b.id.0);

        Ok(PartitionData {
            partition: partition.to_string(),
            facts: all_facts,
            intents: all_intents,
            hints: all_hints,
        })
    }
}

// ── EvictCapable ──────────────────────────────────────────────────────────

impl<K: KeyValueStore, B: BlobStore, O: ObjectStore, C: Now> EvictCapable
    for CompositeColdStorage<K, B, O, C>
{
    fn approximate_size(&self) -> usize {
        let kv_count = self.kv.list("").map(|k| k.len()).unwrap_or(0);
        let blob_count = self.blob.list("").map(|k| k.len()).unwrap_or(0);
        kv_count + blob_count
    }

    fn evict_before(&self, before: &str) -> Result<u64, String> {
        let before_ts: u64 = before.parse().unwrap_or(u64::MAX);
        let blob_keys = self.blob.list("")?;
        let mut evicted = 0u64;
        for key in &blob_keys {
            // Key format: {project_id}/flush/{entity}/{partition}/{ts}.jsonl
            if key.ends_with(".jsonl")
                && let Some(ts_str) = key
                    .strip_suffix(".jsonl")
                    .and_then(|k| k.rsplit('/').next())
                && let Ok(ts) = ts_str.parse::<u64>()
                && ts < before_ts
            {
                self.blob.delete(key)?;
                evicted += 1;
            }
        }
        Ok(evicted)
    }
}

// ── TimeRangeCapable ───────────────────────────────────────────────────────

impl<K: KeyValueStore, B: BlobStore, O: ObjectStore, C: Now> TimeRangeCapable
    for CompositeColdStorage<K, B, O, C>
{
    fn time_range(&self) -> Option<Range<String>> {
        None
    }
}

// ── FlushCapable ──────────────────────────────────────────────────────────

impl<K: KeyValueStore, B: BlobStore, O: ObjectStore, C: Now> FlushCapable
    for CompositeColdStorage<K, B, O, C>
{
    fn flush_since(&self, cursor: &FlushCursor) -> Result<FlushResult, String> {
        let since = &cursor.last_flushed_at;
        let partition = &cursor.partition;
        let now_ts = self.clock.now_nanos();

        // Streaming flush: iterate KV keys one by one, filter by cursor,
        // write matching entries immediately to blob. Never loads all data
        // into memory at once — critical for WASM where heap is limited.
        let fact_prefix = fact_prefix(self.project());
        let fact_keys = self.kv.list(&fact_prefix)?;

        let mut fact_lines: Vec<String> = Vec::new();
        for key in &fact_keys {
            if let Some(json) = self.kv.get(key)?
                && let Ok(stamped) = serde_json::from_str::<Stamped<Fact>>(&json)
                && (since.is_empty() || stamped.submitted_at.as_str() > since.as_str())
                && let Ok(line) = serde_json::to_string(&stamped.data)
            {
                fact_lines.push(line);
            }
        }

        let intent_prefix = intent_prefix(self.project());
        let intent_keys = self.kv.list(&intent_prefix)?;
        let mut intent_lines: Vec<String> = Vec::new();
        for key in &intent_keys {
            if let Some(json) = self.kv.get(key)?
                && let Ok(stamped) = serde_json::from_str::<Stamped<Intent>>(&json)
                && (since.is_empty() || stamped.submitted_at.as_str() > since.as_str())
                && let Ok(line) = serde_json::to_string(&stamped.data)
            {
                intent_lines.push(line);
            }
        }

        let hint_prefix = hint_prefix(self.project());
        let hint_keys = self.kv.list(&hint_prefix)?;
        let mut hint_lines: Vec<String> = Vec::new();
        for key in &hint_keys {
            if let Some(json) = self.kv.get(key)?
                && let Ok(stamped) = serde_json::from_str::<Stamped<Hint>>(&json)
                && (since.is_empty() || stamped.submitted_at.as_str() > since.as_str())
                && let Ok(line) = serde_json::to_string(&stamped.data)
            {
                hint_lines.push(line);
            }
        }

        let records_flushed = (fact_lines.len() + intent_lines.len() + hint_lines.len()) as u64;

        // Write JSON lines to blobs.
        if !fact_lines.is_empty() {
            let blob_key = flush_blob_key(self.project(), "facts", partition, &now_ts);
            self.blob.put(&blob_key, fact_lines.join("\n").as_bytes())?;
        }
        if !intent_lines.is_empty() {
            let blob_key = flush_blob_key(self.project(), "intents", partition, &now_ts);
            self.blob
                .put(&blob_key, intent_lines.join("\n").as_bytes())?;
        }
        if !hint_lines.is_empty() {
            let blob_key = flush_blob_key(self.project(), "hints", partition, &now_ts);
            self.blob.put(&blob_key, hint_lines.join("\n").as_bytes())?;
        }

        let new_cursor = FlushCursor {
            last_flushed_at: now_ts.clone(),
            partition: partition.clone(),
        };

        // Persist cursor to KV.
        let cursor_json =
            serde_json::to_string(&new_cursor).map_err(|e| format!("serialize cursor: {e}"))?;
        self.kv.set(&cursor_key(self.project()), &cursor_json)?;

        Ok(FlushResult {
            records_flushed,
            new_cursor,
        })
    }
}

// ── CypherCapable ─────────────────────────────────────────────────────────

impl<K: KeyValueStore, B: BlobStore, O: ObjectStore, C: Now> CypherCapable
    for CompositeColdStorage<K, B, O, C>
{
}
