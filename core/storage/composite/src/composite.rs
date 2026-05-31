// CompositeColdStorage — platform-independent cold storage backed by a
// BlobStore (blob) + ObjectStore (object) + MetaStore (meta) trio.
//
// # Storage architecture
//
// CompositeColdStorage is the durable persistence layer. It does NOT handle
// in-memory graph operations (FactCapable, IntentCapable, HintCapable) —
// those are delegated to PetgraphStorage (hot storage). Instead, it provides:
//
//   - Blob (R2, S3): Petgraph snapshot archive + flush output
//   - Object (DO): CAS-based claim coordination
//   - Meta (KV): Cursor position, snapshot pointers, delta metadata
//
// External bindings (rs-worker, CF Workers WASM) inject concrete B/O/M
// implementations. CompositeColdStorage itself is fully platform-independent
// and contains no Cloudflare-specific code.
//
// ```
//                 ┌──────────────────────────────────────────────┐
//                 │          ColdStorage trait                    │
//                 │  (ScanCapable + EvictCapable + TimeRangeCapable │
//                 │   + CypherCapable + FlushCapable)             │
//                 └─────────────────────┬────────────────────────┘
//                                       │
//                 ┌─────────────────────┴────────────────────────┐
//                 │        CompositeColdStorage<B, O, M, C>      │
//                 │                                              │
//                 │  ┌──────────┐  ┌──────────┐  ┌────────────┐ │
//                 │  │ Blob     │  │ Object   │  │ Meta       │ │
//                 │  │ archive  │  │ coord.   │  │ cursor/delta│ │
//                 │  └────┬─────┘  └────┬─────┘  └─────┬──────┘ │
//                 └───────┼──────────────┼────────────────┼──────┘
//                         │              │                 │
//                     R2 bucket     Durable Object    Workers KV
//                     filesystem     Redis lock        sled
//                     MockBlob       MockObject        MockKv
// ```
//
// # Tier roles
//
// | Tier | Store | Role | Write | Read |
// |------|-------|------|-------|------|
// | 1 | Blob | Petgraph snapshot archive + flush output | `flush_since` | bulk `scan_partition` |
// | 2 | Object | CAS-based claim coordination | `compare_and_swap` | `get_state` |
// | 3 | Meta | Cursor position, snapshot pointers | `set(key, value)` | `get(key)` |
//
// Graph CRUD (FactCapable, IntentCapable, HintCapable, StorageRead) is
// handled by PetgraphStorage (hot storage), NOT by CompositeColdStorage.
// CompositeColdStorage only manages durable persistence.

use crate::{BlobStore, MetaStore, ObjectStore, flush_blob_prefix};
use crate::{Now, SystemClock};
use log;
use nexus_graph::CypherCapable;
use nexus_model::{
    BoardState, ColdStorage, EvictCapable, FlushCapable, FlushCursor, FlushResult, PartitionData,
    ScanCapable, StorageRead, TimeRangeCapable,
};
use postcard;
use std::ops::Range;

// ── CompositeColdStorage ──────────────────────────────────────────────────────

/// Cold storage backend backed by a BlobStore + ObjectStore + MetaStore trio.
///
/// Generic over B (BlobStore), O (ObjectStore), and M (MetaStore), allowing
/// the same CompositeColdStorage logic to run in tests (MockBlob + MockObject +
/// MockKv as MetaStore), CF Workers (R2 + Durable Object + KV as MetaStore),
/// or servers (filesystem + Redis lock + sled as MetaStore).
///
/// This does NOT implement `FactCapable`, `IntentCapable`, `HintCapable`, or
/// `StorageRead` — those are handled by PetgraphStorage (hot). This is purely
/// the durable persistence layer.
///
/// # Blob storage layout (produced by flush_since)
///
/// - `{project_id}/flush/{entity}/{partition}/{ts}_{i}.bin`
///
/// # Meta storage layout
///
/// - `cursor` → JSON FlushCursor
/// - `snapshot_ts` → timestamp string
#[derive(Clone)]
pub struct CompositeColdStorage<B: BlobStore, O: ObjectStore, M: MetaStore, C: Now = SystemClock> {
    blob: B,
    object: O,
    meta: M,
    clock: C,
    project_id: String,
}

// ── Constructor ─────────────────────────────────────────────────────────────

impl<B: BlobStore, O: ObjectStore, M: MetaStore, C: Now> CompositeColdStorage<B, O, M, C> {
    pub fn new(blob: B, object: O, meta: M, clock: C, project_id: impl Into<String>) -> Self {
        Self {
            blob,
            object,
            meta,
            clock,
            project_id: project_id.into(),
        }
    }

    /// Access the underlying Blob store.
    pub fn blob(&self) -> &B {
        &self.blob
    }

    /// Access the underlying Object store.
    pub fn object(&self) -> &O {
        &self.object
    }

    /// Access the underlying Meta store (cursor, snapshot pointers).
    pub fn meta(&self) -> &M {
        &self.meta
    }

    // ── Cursor API ──────────────────────────────────────────────────────────

    /// Read the current flush cursor from the meta store.
    /// Returns None if no flush has occurred yet.
    pub fn read_cursor(&self) -> Result<Option<FlushCursor>, String> {
        let cursor_key = format!("{}:cursor", self.project());
        match self.meta.get(&cursor_key)? {
            Some(raw) => {
                let cursor: FlushCursor =
                    postcard::from_bytes(raw.as_bytes()).map_err(|e| e.to_string())?;
                Ok(Some(cursor))
            }
            None => Ok(None),
        }
    }

    /// Project identifier for key scoping.
    fn project(&self) -> &str {
        &self.project_id
    }

    // ── Flushed data readers ───────────────────────────────────────────────-

    fn read_flushed_facts(&self, partition: &str) -> Result<Vec<nexus_model::Fact>, String> {
        let prefix = flush_blob_prefix(self.project(), "facts", partition);
        let keys = self.blob.list(&prefix)?;
        let mut facts = Vec::new();
        for key in &keys {
            match self.blob.get(key) {
                Ok(Some(data)) => {
                    if let Ok(fact) = postcard::from_bytes::<nexus_model::Fact>(&data) {
                        facts.push(fact);
                    }
                }
                Ok(None) => {}
                Err(e) => log::warn!("read_flushed_facts: blob {key}: {e}"),
            }
        }
        facts.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        Ok(facts)
    }

    fn read_flushed_intents(&self, partition: &str) -> Result<Vec<nexus_model::Intent>, String> {
        let prefix = flush_blob_prefix(self.project(), "intents", partition);
        let keys = self.blob.list(&prefix)?;
        let mut intents = Vec::new();
        for key in &keys {
            match self.blob.get(key) {
                Ok(Some(data)) => {
                    if let Ok(intent) = postcard::from_bytes::<nexus_model::Intent>(&data) {
                        intents.push(intent);
                    }
                }
                Ok(None) => {}
                Err(e) => log::warn!("read_flushed_intents: blob {key}: {e}"),
            }
        }
        intents.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        Ok(intents)
    }

    fn read_flushed_hints(&self, partition: &str) -> Result<Vec<nexus_model::Hint>, String> {
        let prefix = flush_blob_prefix(self.project(), "hints", partition);
        let keys = self.blob.list(&prefix)?;
        let mut hints = Vec::new();
        for key in &keys {
            match self.blob.get(key) {
                Ok(Some(data)) => {
                    if let Ok(hint) = postcard::from_bytes::<nexus_model::Hint>(&data) {
                        hints.push(hint);
                    }
                }
                Ok(None) => {}
                Err(e) => log::warn!("read_flushed_hints: blob {key}: {e}"),
            }
        }
        hints.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        Ok(hints)
    }
}

// ── SystemClock convenience constructor ─────────────────────────────────────

impl<B: BlobStore + Clone, O: ObjectStore, M: MetaStore + Clone>
    CompositeColdStorage<B, O, M, SystemClock>
{
    pub fn new_with_system_clock(
        blob: B,
        object: O,
        meta: M,
        project_id: impl Into<String>,
    ) -> Self {
        Self {
            blob,
            object,
            meta,
            clock: SystemClock,
            project_id: project_id.into(),
        }
    }
}

impl<B: BlobStore, O: ObjectStore, M: MetaStore, C: Now> StorageRead
    for CompositeColdStorage<B, O, M, C>
{
    fn project_id(&self) -> &str {
        self.project()
    }

    fn read_state(&self) -> BoardState {
        // CompositeColdStorage no longer stores graph data.
        // StorageRead is implemented only to satisfy trait bounds on
        // FlushCapable, ScanCapable, etc. Return empty state — the
        // actual graph state is managed by PetgraphStorage (hot).
        BoardState {
            facts: Vec::new(),
            intents: Vec::new(),
            hints: Vec::new(),
        }
    }
}

// ── FlushCapable ──────────────────────────────────────────────────────────

impl<B: BlobStore, O: ObjectStore, M: MetaStore, C: Now> FlushCapable
    for CompositeColdStorage<B, O, M, C>
{
    fn flush_since(&self, cursor: &FlushCursor) -> Result<FlushResult, String> {
        let partition = &cursor.partition;
        let now_ts = self.clock.now_nanos();

        let mut records_flushed = 0u64;

        // Flush all available blob entries (data already written by caller).
        // CompositeColdStorage does NOT own graph data — flush is a no-op
        // that updates the cursor only. The caller (DualStorage or Worker)
        // writes Petgraph data to blob before calling flush_since.
        //
        // If data was pre-written to blob, count the records (one per blob).
        let fact_prefix = flush_blob_prefix(self.project(), "facts", partition);
        let fact_keys = self.blob.list(&fact_prefix)?;
        records_flushed += fact_keys.len() as u64;
        let intent_prefix = flush_blob_prefix(self.project(), "intents", partition);
        let intent_keys = self.blob.list(&intent_prefix)?;
        records_flushed += intent_keys.len() as u64;
        let hint_prefix = flush_blob_prefix(self.project(), "hints", partition);
        let hint_keys = self.blob.list(&hint_prefix)?;
        records_flushed += hint_keys.len() as u64;

        // Persist cursor via meta store.
        let new_cursor = FlushCursor {
            last_flushed_at: now_ts,
            partition: partition.clone(),
        };
        let cursor_bytes =
            postcard::to_allocvec(&new_cursor).map_err(|e| format!("serialize cursor: {e}"))?;
        let cursor_key = format!("{}:cursor", self.project());
        self.meta
            .set(&cursor_key, &String::from_utf8_lossy(&cursor_bytes))?;

        Ok(FlushResult {
            records_flushed,
            new_cursor,
        })
    }
}

// ── ScanCapable ───────────────────────────────────────────────────────────

impl<B: BlobStore, O: ObjectStore, M: MetaStore, C: Now> ScanCapable
    for CompositeColdStorage<B, O, M, C>
{
    fn scan_partition(&self, partition: &str) -> Result<PartitionData, String> {
        // CompositeColdStorage only reads flushed data from blob.
        // Recent data is in Petgraph (hot), not here.
        // scan_partition merges flushed blobs — caller is responsible
        // for merging with hot data.
        let facts = self.read_flushed_facts(partition)?;
        let intents = self.read_flushed_intents(partition)?;
        let hints = self.read_flushed_hints(partition)?;

        Ok(PartitionData {
            partition: partition.to_string(),
            facts,
            intents,
            hints,
        })
    }
}

// ── EvictCapable ──────────────────────────────────────────────────────────

impl<B: BlobStore, O: ObjectStore, M: MetaStore, C: Now> EvictCapable
    for CompositeColdStorage<B, O, M, C>
{
    fn approximate_size(&self) -> usize {
        self.blob
            .list(&format!("{}/", self.project()))
            .map(|k| k.len())
            .unwrap_or(0)
    }

    fn evict_before(&self, before: &str) -> Result<u64, String> {
        let before_ts: u64 = before
            .parse()
            .map_err(|e| format!("invalid eviction timestamp '{before}': {e}"))?;
        let blob_keys = self.blob.list(&format!("{}/", self.project()))?;
        let mut evicted = 0u64;
        for key in &blob_keys {
            if key.ends_with(".bin")
                && let Some(ts_prefix) = key
                    .strip_suffix(".bin")
                    .and_then(|k| k.rsplit('/').next())
                    .and_then(|s| s.split('_').next())
                && let Ok(ts) = ts_prefix.parse::<u64>()
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

impl<B: BlobStore, O: ObjectStore, M: MetaStore, C: Now> TimeRangeCapable
    for CompositeColdStorage<B, O, M, C>
{
    fn time_range(&self) -> Option<Range<String>> {
        None
    }
}

// ── CypherCapable ─────────────────────────────────────────────────────────

impl<B: BlobStore, O: ObjectStore, M: MetaStore, C: Now> CypherCapable
    for CompositeColdStorage<B, O, M, C>
{
    // CompositeColdStorage does not support Cypher queries directly.
    // Graph queries are handled by PetgraphStorage (hot).
}

impl<B: BlobStore, O: ObjectStore, M: MetaStore, C: Now> ColdStorage
    for CompositeColdStorage<B, O, M, C>
{
    fn write_blob(&self, key: &str, data: &[u8]) -> Result<(), String> {
        self.blob.put(key, data)
    }
}

// ── Snapshot persistence helper (for Worker restart) ────────────────────────

/// Flush petgraph snapshot to blob storage.
/// Called by the Worker after periodic snapshot creation.
pub fn flush_snapshot_to_blob<B: BlobStore>(
    blob: &B,
    project_id: &str,
    snapshot_bytes: &[u8],
    ts: &str,
) -> Result<(), String> {
    let key = format!("{project_id}/snapshot/{ts}.bin");
    blob.put(&key, snapshot_bytes)
}

/// Load the latest snapshot from blob storage.
/// Returns (timestamp, bytes) if any snapshot exists.
pub fn load_latest_snapshot<B: BlobStore>(
    blob: &B,
    project_id: &str,
) -> Result<Option<(String, Vec<u8>)>, String> {
    let prefix = format!("{project_id}/snapshot/");
    let mut keys = blob.list(&prefix)?;
    keys.sort();
    // Last key is the latest snapshot
    if let Some(latest) = keys.last()
        && let Some(data) = blob.get(latest)?
    {
        let ts = latest
            .strip_prefix(&prefix)
            .and_then(|s| s.strip_suffix(".bin"))
            .unwrap_or("")
            .to_string();
        return Ok(Some((ts, data)));
    }
    Ok(None)
}
