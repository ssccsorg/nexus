// nexus-storage-composite — HybridBlackboard: the composite blackboard struct.
//
// Part of the graph runtime. Combines a hot petgraph (for low-latency
// access and Cypher queries) with a cold storage backend for durability.
// Storage is swappable via DualStorage.

use cfg_if::cfg_if;
use nexus_model::{
    BlackboardError, BoardState, ColdStorage, DualStorage, EvictCapable, Fact, FactCapable,
    FihHash, FlushCapable, FlushCursor, FlushResult, Hint, HintCapable, Intent, IntentCapable,
    NullStorage, PartitionData, ScanCapable, StorageRead,
};
use nexus_storage_petgraph::{
    PetgraphStorage, SharedGraph, Snapshottable, StorageSnapshot, read_graph,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

use nexus_model::storage::{EdgeWeight, NodeWeight};

cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        use std::cell::Ref;
        /// WASM: read guard for the shared graph.
        pub type ReadGuard<'a> = Ref<'a, petgraph::Graph<NodeWeight, EdgeWeight>>;
    } else {
        use std::sync::RwLockReadGuard;
        /// Native: read guard for the shared graph.
        pub type ReadGuard<'a> = RwLockReadGuard<'a, petgraph::Graph<NodeWeight, EdgeWeight>>;
    }
}

/// A single query result row from Cypher.
pub type Record = std::collections::HashMap<String, nexus_model::Content>;

/// Tracks intent claims — which agent has claimed which intent.
///
/// Acts as a local cache in front of the storage layer's claim tracking.
/// The storage layer is the source of truth; this cache provides fast
/// conflict detection without a storage round-trip.
#[derive(Clone, Serialize, Deserialize)]
struct ClaimsTracker {
    pub(crate) inner: HashMap<String, String>,
}

#[allow(dead_code)]
impl ClaimsTracker {
    fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    fn try_claim(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        if let Some(current) = self.inner.get(intent_id) {
            return Err(BlackboardError::Conflict(format!(
                "Intent {intent_id} already claimed by {current}"
            )));
        }
        self.inner.insert(intent_id.to_string(), agent.to_string());
        Ok(())
    }

    fn verify_owner(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        match self.inner.get(intent_id) {
            Some(current) if current != agent => Err(BlackboardError::Conflict(format!(
                "Intent {intent_id} is claimed by {current}, not {agent}"
            ))),
            _ => Ok(()),
        }
    }

    fn remove(&mut self, intent_id: &str) {
        self.inner.remove(intent_id);
    }
}

impl ClaimsTracker {
    fn to_snapshot(&self) -> HashMap<String, String> {
        self.inner.clone()
    }

    fn from_snapshot(inner: HashMap<String, String>) -> Self {
        Self { inner }
    }

    fn release(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        match self.inner.get(intent_id) {
            None => {}
            Some(current) if current != agent => {
                return Err(BlackboardError::Conflict(format!(
                    "Intent {intent_id} is claimed by {current}, not {agent}"
                )));
            }
            _ => {
                self.inner.remove(intent_id);
            }
        }
        Ok(())
    }
}

/// The composite Blackboard struct. Combines a hot petgraph (for low-latency
/// access and Cypher queries) with a cold storage backend for durability.
///
/// **Lock-free by default**: `claims` has no Mutex — single-worker ownership.
/// `hot_graph` is shared with `PetgraphStorage`. On native it uses
/// `Arc<RwLock<>>` for thread safety; on WASM it uses `Rc<RefCell<>>`
/// since WASM is single-threaded.
///
/// For multi-worker thread-safe access, wrap in `Arc<Mutex<HybridBlackboard>>`.
pub struct HybridBlackboard {
    pub storage: DualStorage,
    pub hot_graph: SharedGraph,
    claims: Mutex<ClaimsTracker>,
    project_id: String,
    /// Last flush position. Persisted via StorageSnapshot.flush_cursor
    /// and restored on worker restart.
    pub flush_cursor: FlushCursor,
}

#[allow(dead_code)]
impl HybridBlackboard {
    pub fn new() -> Self {
        let hot = PetgraphStorage::with_project_id("default");
        let hot_graph = hot.graph.clone();
        let project_id = hot.project_id.clone();
        let storage = DualStorage::new(Box::new(hot), Box::new(NullStorage));

        Self {
            storage,
            hot_graph,
            claims: Mutex::new(ClaimsTracker::new()),
            project_id,
            flush_cursor: FlushCursor::default(),
        }
    }

    pub fn with_storage(hot: PetgraphStorage, cold: Box<dyn ColdStorage>) -> Self {
        let hot_graph = hot.graph.clone();
        let project_id = hot.project_id.clone();
        let storage = DualStorage::new(Box::new(hot), cold);

        Self {
            storage,
            hot_graph,
            claims: Mutex::new(ClaimsTracker::new()),
            project_id,
            flush_cursor: FlushCursor::default(),
        }
    }

    pub fn with_graph<R>(
        &self,
        f: impl FnOnce(&petgraph::Graph<NodeWeight, EdgeWeight>) -> R,
    ) -> R {
        let g = read_graph(&self.hot_graph);
        f(&g)
    }

    /// Returns a guard to the hot petgraph for direct query access.
    /// The guard implements `GraphRead`, so callers can pass it to
    /// `interface_cypher::execute*` functions directly.
    ///
    /// For cold-backed query execution, use `interface_cypher::execute_with_cold`
    /// with the graph guard and your cold backend:
    ///
    /// ```ignore
    /// let guard = bb.graph();
    /// let records = interface_cypher::execute_with_cold(&guard, &cold_storage, &plan)?;
    /// ```
    ///
    /// The return type varies by platform:
    /// - Native: `RwLockReadGuard<'_, Graph>`
    /// - WASM: `Ref<'_, Graph>`
    ///
    /// Both implement `GraphRead` and `Deref<Target=Graph>`.
    pub fn graph(&self) -> ReadGuard<'_> {
        read_graph(&self.hot_graph)
    }

    /// Flush recently-ingested data to cold storage.
    ///
    /// Reads the last flush cursor, passes it to the cold backend's
    /// `flush_since`, and persists the updated cursor for incremental
    /// export on the next call.
    ///
    /// cold backend determines what "flush" means:
    /// - `NullStorage`: no-op (no cold storage configured)
    /// - `CompositeColdStorage`: writes hot delta to blob, advances cursor
    /// - `DuckDbStorage`: exports hot data newer than cursor to Parquet files
    /// - Future backends: their own incremental export semantics
    pub fn flush(&mut self) -> Result<(), String> {
        let FlushResult {
            records_flushed: _,
            new_cursor,
        } = self.storage.flush_since(&self.flush_cursor)?;
        self.flush_cursor = new_cursor;
        Ok(())
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }

    /// Returns a guard to the hot petgraph for query access.
    ///
    /// The return type varies by platform:
    /// - Native: `RwLockReadGuard<'_, Graph>`
    /// - WASM: `Ref<'_, Graph>`
    ///
    /// Both implement `GraphRead` and `Deref<Target=Graph>`.
    pub fn snapshot(&self) -> ReadGuard<'_> {
        read_graph(&self.hot_graph)
    }

    /// Serialise the current state for blob storage (R2, S3, etc.).
    /// Clones the graph and claims — use sparingly, not on every iteration.
    pub fn to_snapshot(&self) -> StorageSnapshot {
        let g = read_graph(&self.hot_graph);
        StorageSnapshot {
            graph: g.clone(),
            claims: self.claims.lock().unwrap().to_snapshot(),
            project_id: self.project_id.clone(),
            task_states: std::collections::HashMap::new(),
            flush_cursor: self.flush_cursor.clone(),
            version: 1,
        }
    }

    /// Reconstruct a `HybridBlackboard` from a previously saved snapshot.
    ///
    /// Internal helper used by `Snapshottable::from_snapshot`.
    /// Creates a fresh `PetgraphStorage + NullStorage` pair; the caller may
    /// replace the cold backend via `with_storage()` afterward.
    fn from_snapshot_inner(snapshot: &StorageSnapshot) -> Self {
        let graph_data = snapshot.graph.clone();
        let hot = PetgraphStorage::with_shared_graph_from_data(graph_data, &snapshot.project_id);
        let hot_graph = hot.graph.clone();
        let project_id = hot.project_id.clone();
        let storage = DualStorage::new(Box::new(hot), Box::new(NullStorage));

        Self {
            storage,
            hot_graph,
            claims: Mutex::new(ClaimsTracker::from_snapshot(snapshot.claims.clone())),
            project_id,
            flush_cursor: snapshot.flush_cursor.clone(),
        }
    }

    /// Reconstruct from a snapshot with a caller-provided cold backend.
    /// The hot petgraph graph, flush cursor, and claims are restored from
    /// the snapshot; the cold storage is supplied externally (e.g. a fresh
    /// CompositeColdStorage after worker restart).
    pub fn from_snapshot_with_cold(snapshot: &StorageSnapshot, cold: Box<dyn ColdStorage>) -> Self {
        let graph_data = snapshot.graph.clone();
        let hot = PetgraphStorage::with_shared_graph_from_data(graph_data, &snapshot.project_id);
        let hot_graph = hot.graph.clone();
        let project_id = hot.project_id.clone();
        let storage = DualStorage::new(Box::new(hot), cold);

        Self {
            storage,
            hot_graph,
            claims: Mutex::new(ClaimsTracker::from_snapshot(snapshot.claims.clone())),
            project_id,
            flush_cursor: snapshot.flush_cursor.clone(),
        }
    }
}

impl Default for HybridBlackboard {
    fn default() -> Self {
        Self::new()
    }
}

// GraphRead / GraphWrite are NOT implemented for HybridBlackboard.
// Use snapshot() or graph() for a guard that implements GraphRead.

// ── StorageRead — delegates to hot storage ────────────────────────────────

impl StorageRead for HybridBlackboard {
    fn project_id(&self) -> &str {
        &self.project_id
    }

    fn read_state(&self) -> BoardState {
        self.storage.read_state()
    }
}

// ── Eviction support — delegates to storage ───────────────────────────────

impl EvictCapable for HybridBlackboard {
    fn approximate_size(&self) -> usize {
        EvictCapable::approximate_size(&self.storage)
    }

    fn evict_before(&self, before: &str) -> Result<u64, String> {
        EvictCapable::evict_before(&self.storage, before)
    }

    fn evict_stale_intents(&self, older_than_secs: u64) -> Result<u64, String> {
        EvictCapable::evict_stale_intents(&self.storage, older_than_secs)
    }
}

// ── Flush — delegates to storage (DualStorage → cold) ────────────────────

impl FlushCapable for HybridBlackboard {
    fn flush_since(&self, cursor: &FlushCursor) -> Result<FlushResult, String> {
        self.storage.flush_since(cursor)
    }
}

impl ScanCapable for HybridBlackboard {
    fn scan_partition(&self, partition: &str) -> Result<PartitionData, String> {
        self.storage.scan_partition(partition)
    }
}

// ── FactCapable — delegates to storage ───────────────────────────────────

impl FactCapable for HybridBlackboard {
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        self.storage.submit_fact(fact)
    }
}

// ── HintCapable — delegates to storage ───────────────────────────────────

impl HintCapable for HybridBlackboard {
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        self.storage.submit_hint(hint)
    }
}

// ── IntentCapable — full lifecycle with local claims tracking ────────────

impl IntentCapable for HybridBlackboard {
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        self.storage.submit_intent(intent)
    }

    fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.claims.lock().unwrap().try_claim(intent_id, agent)?;
        match self.storage.claim_intent(intent_id, agent) {
            Ok(()) => Ok(()),
            Err(e) => {
                self.claims.lock().unwrap().remove(intent_id);
                Err(e)
            }
        }
    }

    fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.claims.lock().unwrap().verify_owner(intent_id, agent)?;
        self.storage.heartbeat(intent_id, agent)
    }

    fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.claims.lock().unwrap().release(intent_id, agent)?;
        self.storage.release_intent(intent_id, agent)
    }

    fn conclude_intent(&self, intent_id: &str, result: &str) -> Result<Fact, BlackboardError> {
        self.claims.lock().unwrap().remove(intent_id);
        self.storage.conclude_intent(intent_id, result)
    }
}

impl Snapshottable for HybridBlackboard {
    fn to_snapshot(&self) -> StorageSnapshot {
        HybridBlackboard::to_snapshot(self)
    }

    fn from_snapshot(snapshot: StorageSnapshot) -> Self {
        HybridBlackboard::from_snapshot_inner(&snapshot)
    }
}
