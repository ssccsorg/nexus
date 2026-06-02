// nexus-graph — DefaultBlackboard: the single blackboard struct.
//
// Part of the graph runtime. Combines a hot petgraph (for low-latency
// access and Cypher queries) with a cold storage backend for durability.
// Storage is swappable via DualStorage.

use crate::storage::petgraph::{PetgraphStorage, Snapshottable, StorageSnapshot};
use nexus_model::{
    Blackboard, BlackboardError, BoardState, ColdStorage, DualStorage, EvictCapable, Fact,
    FactCapable, FihHash, FlushCapable, FlushCursor, FlushResult, Hint, HintCapable, Intent,
    IntentCapable, NullStorage, PartitionData, ScanCapable, StorageRead,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use nexus_model::storage::{EdgeWeight, NodeWeight};

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

/// RAII guard: releases claim on drop if not committed.
/// Prevents stale claims when the caller panics or forgets to conclude.
///
/// Holds `&mut ClaimsTracker` directly — no re-lock needed on drop
/// since `DefaultBlackboard.claims` is lock-free.
struct ClaimGuard<'a> {
    claims: &'a mut ClaimsTracker,
    intent_id: String,
    agent: String,
    committed: bool,
}

impl<'a> ClaimGuard<'a> {
    fn new(claims: &'a mut ClaimsTracker, intent_id: String, agent: String) -> Self {
        Self {
            claims,
            intent_id,
            agent,
            committed: false,
        }
    }

    fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for ClaimGuard<'_> {
    fn drop(&mut self) {
        if !self.committed {
            let _ = self.claims.release(&self.intent_id, &self.agent);
        }
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

/// The single Blackboard struct. Combines a hot petgraph (for low-latency
/// access and Cypher queries) with a cold storage backend for durability.
///
/// **Lock-free by default**: `claims` has no Mutex — single-worker ownership.
/// `hot_graph` is shared with `PetgraphStorage` via `Arc<RwLock<>>` but
/// the lock is internal to the storage layer, not this struct.
///
/// For multi-worker thread-safe access, wrap in `Arc<Mutex<DefaultBlackboard>>`.
pub struct DefaultBlackboard {
    pub storage: DualStorage,
    pub hot_graph: Arc<RwLock<petgraph::Graph<NodeWeight, EdgeWeight>>>,
    claims: ClaimsTracker,
    project_id: String,
    /// Last flush position. Persisted via StorageSnapshot.flush_cursor
    /// and restored on worker restart.
    pub flush_cursor: FlushCursor,
}

#[allow(dead_code)]
impl DefaultBlackboard {
    pub fn new() -> Self {
        let graph = Arc::new(RwLock::new(petgraph::Graph::new()));
        let hot = Box::new(PetgraphStorage::with_shared_graph(
            Arc::clone(&graph),
            "default",
        ));
        let cold = Box::new(NullStorage);
        let storage = DualStorage::new(hot, cold);

        Self {
            storage,
            hot_graph: graph,
            claims: ClaimsTracker::new(),
            project_id: "default".into(),
            flush_cursor: FlushCursor::default(),
        }
    }

    pub fn with_storage(hot: PetgraphStorage, cold: Box<dyn ColdStorage>) -> Self {
        let hot_graph = Arc::clone(&hot.graph);
        let project_id = hot.project_id.clone();
        let storage = DualStorage::new(Box::new(hot), cold);

        Self {
            storage,
            hot_graph,
            claims: ClaimsTracker::new(),
            project_id,
            flush_cursor: FlushCursor::default(),
        }
    }

    pub fn with_graph<R>(
        &self,
        f: impl FnOnce(&petgraph::Graph<NodeWeight, EdgeWeight>) -> R,
    ) -> R {
        let g = self.hot_graph.read().unwrap();
        f(&g)
    }

    /// Returns an `RwLockReadGuard` to the hot petgraph for direct query access.
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
    pub fn graph(&self) -> std::sync::RwLockReadGuard<'_, petgraph::Graph<NodeWeight, EdgeWeight>> {
        self.hot_graph.read().unwrap()
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

    pub fn snapshot(
        &self,
    ) -> std::sync::RwLockReadGuard<'_, petgraph::Graph<NodeWeight, EdgeWeight>> {
        self.hot_graph.read().unwrap()
    }

    /// Serialise the current state for blob storage (R2, S3, etc.).
    /// Clones the graph and claims — use sparingly, not on every iteration.
    pub fn to_snapshot(&self) -> StorageSnapshot {
        let g = self.hot_graph.read().unwrap();
        StorageSnapshot {
            graph: g.clone(),
            claims: self.claims.to_snapshot(),
            project_id: self.project_id.clone(),
            task_states: std::collections::HashMap::new(),
            flush_cursor: self.flush_cursor.clone(),
            version: 1,
        }
    }

    /// Reconstruct a `DefaultBlackboard` from a previously saved snapshot.
    ///
    /// Internal helper used by `Snapshottable::from_snapshot`.
    /// Creates a fresh `PetgraphStorage + NullStorage` pair; the caller may
    /// replace the cold backend via `with_storage()` afterward.
    fn from_snapshot_inner(snapshot: &StorageSnapshot) -> Self {
        let graph = Arc::new(RwLock::new(snapshot.graph.clone()));
        let hot = Box::new(PetgraphStorage::with_shared_graph(
            Arc::clone(&graph),
            &snapshot.project_id,
        ));
        let cold = Box::new(NullStorage);
        let storage = DualStorage::new(hot, cold);

        Self {
            storage,
            hot_graph: graph,
            claims: ClaimsTracker::from_snapshot(snapshot.claims.clone()),
            project_id: snapshot.project_id.clone(),
            flush_cursor: snapshot.flush_cursor.clone(),
        }
    }

    /// Reconstruct from a snapshot with a caller-provided cold backend.
    /// The hot petgraph graph, flush cursor, and claims are restored from
    /// the snapshot; the cold storage is supplied externally (e.g. a fresh
    /// CompositeColdStorage after worker restart).
    pub fn from_snapshot_with_cold(snapshot: &StorageSnapshot, cold: Box<dyn ColdStorage>) -> Self {
        let graph = Arc::new(RwLock::new(snapshot.graph.clone()));
        let hot = Box::new(PetgraphStorage::with_shared_graph(
            Arc::clone(&graph),
            &snapshot.project_id,
        ));
        let storage = DualStorage::new(hot, cold);

        Self {
            storage,
            hot_graph: graph,
            claims: ClaimsTracker::from_snapshot(snapshot.claims.clone()),
            project_id: snapshot.project_id.clone(),
            flush_cursor: snapshot.flush_cursor.clone(),
        }
    }
}

impl Default for DefaultBlackboard {
    fn default() -> Self {
        Self::new()
    }
}

// GraphRead and GraphWrite are NOT implemented for DefaultBlackboard
// because the trait requires returning an &petgraph::Graph, which
// cannot be acquired from an Arc<RwLock<...>> without runtime locking.
// Callers should use DefaultBlackboard::snapshot() which returns an
// RwLockReadGuard (implementing GraphRead) for query access.

// ── StorageRead — delegates to hot storage ────────────────────────────────

impl StorageRead for DefaultBlackboard {
    fn project_id(&self) -> &str {
        &self.project_id
    }

    fn read_state(&self) -> BoardState {
        self.storage.read_state()
    }
}

// ── Eviction support — delegates to storage ───────────────────────────────

impl EvictCapable for DefaultBlackboard {
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

impl FlushCapable for DefaultBlackboard {
    fn flush_since(&self, cursor: &FlushCursor) -> Result<FlushResult, String> {
        self.storage.flush_since(cursor)
    }
}

impl ScanCapable for DefaultBlackboard {
    fn scan_partition(&self, partition: &str) -> Result<PartitionData, String> {
        self.storage.scan_partition(partition)
    }
}

impl Blackboard for DefaultBlackboard {
    fn project_id(&self) -> &str {
        &self.project_id
    }

    fn submit_fact(&mut self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        self.storage.submit_fact(fact)
    }

    fn submit_hint(&mut self, hint: &Hint) -> Result<(), BlackboardError> {
        self.storage.submit_hint(hint)
    }

    fn submit_intent(&mut self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        self.storage.submit_intent(intent)
    }

    fn claim_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.claims.try_claim(intent_id, agent)?;
        let mut guard = ClaimGuard::new(&mut self.claims, intent_id.to_string(), agent.to_string());
        self.storage.claim_intent(intent_id, agent)?;
        guard.commit();
        Ok(())
    }

    fn heartbeat(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.claims.verify_owner(intent_id, agent)?;
        self.storage.heartbeat(intent_id, agent)
    }

    fn release_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.claims.release(intent_id, agent)?;
        self.storage.release_intent(intent_id, agent)
    }

    fn conclude_intent(&mut self, intent_id: &str, result: &str) -> Result<Fact, BlackboardError> {
        self.claims.remove(intent_id);
        self.storage.conclude_intent(intent_id, result)
    }

    fn read_state(&self) -> BoardState {
        self.storage.read_state()
    }
}

impl Snapshottable for DefaultBlackboard {
    fn to_snapshot(&self) -> StorageSnapshot {
        DefaultBlackboard::to_snapshot(self)
    }

    fn from_snapshot(snapshot: StorageSnapshot) -> Self {
        DefaultBlackboard::from_snapshot_inner(&snapshot)
    }
}
