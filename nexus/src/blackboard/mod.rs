// nexus-graph — DefaultBlackboard: the single blackboard struct.
//
// Part of the graph runtime. Combines a hot petgraph (for low-latency
// access and Cypher queries) with a cold storage backend for durability.
// Storage is swappable via DualStorage.

use crate::storage::petgraph::{
    EdgeWeight, GraphRead, GraphWrite, NodeWeight, PetgraphStorage, Snapshottable, StorageSnapshot,
};
use interface_cypher::capable::CypherCapable;
use interface_cypher::{Plan, TranslateError, execute_with_cold};
use nexus_model::{
    Blackboard, BlackboardError, BoardState, ColdStorage, DualStorage, EvictCapable, Fact,
    FactCapable, FihHash, FlushCapable, FlushCursor, FlushResult, Hint, HintCapable, Intent,
    IntentCapable, NullStorage, PartitionData, ScanCapable, StorageRead,
};
use petgraph::graph::{EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

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
    storage: DualStorage,
    /// Cold storage backend that implements CypherCapable.
    /// Holds the same cold storage instance as DualStorage's cold backend.
    cold_cypher: Box<dyn CypherCapable>,
    hot_graph: Arc<RwLock<petgraph::Graph<NodeWeight, EdgeWeight>>>,
    claims: ClaimsTracker,
    project_id: String,
    /// Last flush position. Persisted via StorageSnapshot.flush_cursor
    /// and restored on worker restart.
    pub(crate) flush_cursor: FlushCursor,
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
        let cold_cypher: Box<dyn CypherCapable> = Box::new(NullStorage);
        let storage = DualStorage::new(hot, cold);

        Self {
            storage,
            cold_cypher,
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

        // Clone the cold backend for CypherCapable use.
        // Since ColdStorage is the only trait we know, we need to downcast.
        // For now, use NullStorage as fallback.
        let cold_cypher: Box<dyn CypherCapable> = Box::new(NullStorage);

        Self {
            storage,
            cold_cypher,
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

    /// Execute a Cypher query plan with hot/cold routing.
    ///
    /// Hot queries run against the in-memory petgraph (µs).
    /// Cold-eligible queries (simple tabular scans) route to the cold storage
    /// backend (DuckDB/Parquet) via the `CypherCapable` trait.
    pub fn query(&self, plan: &Plan) -> Result<Vec<Record>, TranslateError> {
        let hot = self.hot_graph.read().unwrap();
        execute_with_cold(&*hot, &*self.cold_cypher, plan)
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
        let cold_cypher: Box<dyn CypherCapable> = Box::new(NullStorage);
        let storage = DualStorage::new(hot, cold);

        Self {
            storage,
            cold_cypher,
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
        // The caller is responsible for providing a matching CypherCapable backend.
        // Fall back to NullStorage if none is available.
        let cold_cypher: Box<dyn CypherCapable> = Box::new(NullStorage);

        Self {
            storage,
            cold_cypher,
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

impl GraphRead for DefaultBlackboard {
    fn node_indices(&self) -> Vec<NodeIndex> {
        let g = self.hot_graph.read().unwrap();
        petgraph::Graph::node_indices(&*g).collect()
    }

    fn edge_indices(&self) -> Vec<EdgeIndex> {
        let g = self.hot_graph.read().unwrap();
        petgraph::Graph::edge_indices(&*g).collect()
    }

    fn node_weight(&self, idx: NodeIndex) -> Option<NodeWeight> {
        let g = self.hot_graph.read().unwrap();
        petgraph::Graph::node_weight(&*g, idx).cloned()
    }

    fn edge_weight(&self, idx: EdgeIndex) -> Option<EdgeWeight> {
        let g = self.hot_graph.read().unwrap();
        petgraph::Graph::edge_weight(&*g, idx).cloned()
    }

    fn edge_endpoints(&self, idx: EdgeIndex) -> Option<(NodeIndex, NodeIndex)> {
        let g = self.hot_graph.read().unwrap();
        petgraph::Graph::edge_endpoints(&*g, idx)
    }

    fn neighbors_undirected(&self, idx: NodeIndex) -> Vec<NodeIndex> {
        let g = self.hot_graph.read().unwrap();
        petgraph::Graph::neighbors_undirected(&*g, idx).collect()
    }

    fn edges_directed(&self, idx: NodeIndex, outgoing: bool) -> Vec<EdgeIndex> {
        let g = self.hot_graph.read().unwrap();
        let dir = if outgoing {
            petgraph::Direction::Outgoing
        } else {
            petgraph::Direction::Incoming
        };
        petgraph::Graph::edges_directed(&*g, idx, dir)
            .map(|e| e.id())
            .collect()
    }
}

impl GraphWrite for DefaultBlackboard {
    fn add_node(&mut self, weight: NodeWeight) -> NodeIndex {
        let mut g = self.hot_graph.write().unwrap();
        g.add_node(weight)
    }

    fn add_edge(&mut self, from: NodeIndex, to: NodeIndex, weight: EdgeWeight) -> EdgeIndex {
        let mut g = self.hot_graph.write().unwrap();
        g.add_edge(from, to, weight)
    }
}

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

// ── Cypher query — delegates to the cold CypherCapable backend ────────────

impl CypherCapable for DefaultBlackboard {
    fn query_plan(&self, plan: &serde_json::Value) -> Result<serde_json::Value, String> {
        self.cold_cypher.query_plan(plan)
    }
}

impl CypherCapable for PetgraphStorage {}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DefaultBlackboard;
    use nexus_model::{
        Blackboard, BoardState, Fact, FihHash, FlushCapable, FlushCursor, FlushResult, Intent,
        StorageRead,
    };

    fn tick() {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    fn bb_with_facts() -> DefaultBlackboard {
        let mut bb = DefaultBlackboard::new();
        for i in 0..5 {
            let fact = Fact {
                id: FihHash(format!("f{i}")),
                origin: "test".into(),
                content: format!("fact #{i}").into(),
                creator: "tester".into(),
            };
            bb.submit_fact(&fact).unwrap();
        }
        bb
    }

    #[test]
    fn test_fresh_blackboard_has_empty_cursor() {
        let bb = DefaultBlackboard::new();
        assert_eq!(bb.flush_cursor, FlushCursor::default());
    }

    #[test]
    fn test_flush_updates_cursor() {
        let mut bb = bb_with_facts();
        let before = bb.flush_cursor.clone();
        bb.flush().unwrap();
        let after = bb.flush_cursor.clone();
        assert!(
            after > before,
            "flush should advance cursor: before={before:?}, after={after:?}"
        );
    }

    #[test]
    fn test_consecutive_flushes_advance_cursor() {
        let mut bb = bb_with_facts();
        bb.flush().unwrap();
        let c1 = bb.flush_cursor.clone();

        // add more facts
        for i in 5..10 {
            let fact = Fact {
                id: FihHash(format!("f{i}")),
                origin: "test".into(),
                content: format!("fact #{i}").into(),
                creator: "tester".into(),
            };
            bb.submit_fact(&fact).unwrap();
        }
        tick();

        bb.flush().unwrap();
        let c2 = bb.flush_cursor.clone();
        assert!(
            c2 > c1,
            "second flush should advance cursor: c1={c1:?}, c2={c2:?}"
        );
    }

    #[test]
    fn test_cursor_survives_snapshot_roundtrip() {
        let mut bb = bb_with_facts();
        bb.flush().unwrap();
        let cursor_before = bb.flush_cursor.clone();

        let snap = bb.to_snapshot();
        let restored = DefaultBlackboard::from_snapshot(snap);
        assert_eq!(
            restored.flush_cursor, cursor_before,
            "flush cursor should survive snapshot roundtrip"
        );
    }

    #[test]
    fn test_old_snapshot_without_cursor_gets_default() {
        // Simulate a snapshot created by older code that did not include flush_cursor.
        // The default FlushCursor is the epoch — a fresh blackboard with no cursor
        // should start from the beginning.
        let bb = DefaultBlackboard::new();
        assert_eq!(bb.flush_cursor, FlushCursor::default());
    }

    #[test]
    fn test_flush_after_restore_continues_from_cursor() {
        let mut bb = bb_with_facts();
        bb.flush().unwrap();
        let cursor1 = bb.flush_cursor.clone();

        let snap = bb.to_snapshot();
        let mut restored = DefaultBlackboard::from_snapshot(snap);
        assert_eq!(restored.flush_cursor, cursor1);

        // Add more facts to the original, restore to fresh instance.
        for i in 10..15 {
            let fact = Fact {
                id: FihHash(format!("f{i}")),
                origin: "test".into(),
                content: format!("fact #{i}").into(),
                creator: "tester".into(),
            };
            restored.submit_fact(&fact).unwrap();
        }
        tick();

        restored.flush().unwrap();
        let cursor2 = restored.flush_cursor.clone();
        assert!(
            cursor2 > cursor1,
            "flush after restore should advance from restored cursor: old={cursor1:?}, new={cursor2:?}"
        );
    }

    #[test]
    fn test_cursor_independent_of_graph_mutations() {
        let mut bb = DefaultBlackboard::new();
        let fact = Fact {
            id: FihHash("f1".into()),
            origin: "test".into(),
            content: "test".into(),
            creator: "tester".into(),
        };
        bb.submit_fact(&fact).unwrap();
        bb.flush().unwrap();
        let c1 = bb.flush_cursor.clone();

        // Mutate graph directly (no fact submission).
        {
            let mut g = bb.hot_graph.write().unwrap();
            g.add_node(crate::storage::petgraph::NodeWeight {
                name: "test_node".into(),
                label: "Test".into(),
                properties: HashMap::new(),
            });
        }
        let c2 = bb.flush_cursor.clone();
        assert_eq!(
            c1, c2,
            "direct graph mutation should not advance flush cursor"
        );
    }

    #[test]
    fn test_independent_blackboards_independent_cursors() {
        let mut bb1 = bb_with_facts();
        let mut bb2 = bb_with_facts();
        bb1.flush().unwrap();
        let c1 = bb1.flush_cursor.clone();
        // bb2 hasn't flushed yet — its cursor is still default.
        assert_eq!(bb2.flush_cursor, FlushCursor::default());
        bb2.flush().unwrap();
        assert!(
            bb2.flush_cursor > c1,
            "independent blackboard should have independent cursor"
        );
    }

    #[test]
    fn test_flush_noop_backend() {
        // NullStorage cold backend — flush should succeed.
        let mut bb = bb_with_facts();
        let result = bb.storage.flush_since(&bb.flush_cursor);
        assert!(result.is_ok());
    }

    #[test]
    fn test_flush_cycle_with_facts() {
        let mut bb = bb_with_facts();

        // Cycle flush multiple times.
        for _ in 0..3 {
            tick();
            let _ = bb.flush();
        }
    }

    #[test]
    fn test_flush_empty_blackboard() {
        let mut bb = DefaultBlackboard::new();
        bb.flush().unwrap();
    }

    #[test]
    fn test_cursor_timestamp_numeric() {
        let mut bb = bb_with_facts();
        bb.flush().unwrap();
        let cursor = bb.flush_cursor.clone();
        let ts: u128 = cursor.last_flushed_at.parse().unwrap_or(0);
        assert!(ts > 0, "flush cursor should contain a positive timestamp");
    }

    #[test]
    fn test_storage_snapshot_roundtrip() {
        use crate::storage::petgraph::Snapshottable;

        let mut bb = bb_with_facts();
        // Add intents
        let intent = Intent {
            id: FihHash("i1".into()),
            from_facts: vec![],
            to_fact_id: None,
            description: "test goal".into(),
            creator: "tester".into(),
            worker: None,
            last_heartbeat_at: None,
            created_at: None,
            concluded_at: None,
        };
        bb.submit_intent(&intent).unwrap();

        let snapshot = bb.to_snapshot();

        // Reconstruct
        let restored = DefaultBlackboard::from_snapshot(snapshot);

        // Verify graph data
        let state = <DefaultBlackboard as nexus_model::Blackboard>::read_state(&restored);
        assert_eq!(state.facts.len(), 5);
        assert_eq!(state.intents.len(), 1);

        // Verify project_id
        assert_eq!(restored.project_id(), "default");
    }
}
