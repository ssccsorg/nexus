// nexus-graph — DefaultBlackboard: the single blackboard struct.
//
// Part of the graph runtime. Combines a hot petgraph (for low-latency
// access and Cypher queries) with a cold storage backend for durability.
// Storage is swappable via DualStorage.

use crate::query::cypher::{Plan, TranslateError, execute_with_cold};
use nexus_model::{
    Blackboard, BlackboardError, BoardState, ColdStorage, CypherCapable, DualStorage, EvictCapable,
    Fact, FactCapable, FihHash, FlushCapable, FlushCursor, FlushResult, Hint, HintCapable, Intent,
    IntentCapable, NullStorage, StorageRead,
};
use nexus_storage_petgraph::{
    EdgeWeight, GraphRead, GraphWrite, NodeWeight, PetgraphStorage, Record, Snapshottable,
    StorageSnapshot,
};
use petgraph::graph::{EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

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
pub(crate) struct DefaultBlackboard {
    storage: DualStorage,
    hot_graph: Arc<RwLock<petgraph::Graph<NodeWeight, EdgeWeight>>>,
    claims: ClaimsTracker,
    project_id: String,
    /// Last flush position. Persisted via StorageSnapshot.flush_cursor
    /// and restored on worker restart. No metadata node needed.
    flush_cursor: FlushCursor,
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

    /// Execute a Cypher query plan with hot/cold routing.
    ///
    /// Hot queries run against the in-memory petgraph (µs).
    /// Cold-eligible queries (simple tabular scans) route to the cold storage
    /// backend (DuckDB/Parquet) via the `CypherCapable` trait.
    pub fn query(&self, plan: &Plan) -> Result<Vec<Record>, TranslateError> {
        let hot = self.hot_graph.read().unwrap();
        execute_with_cold(&*hot, &self.storage, plan)
    }

    /// Flush recently-ingested data to cold storage.
    ///
    /// Reads the last flush cursor, passes it to the cold backend's
    /// `flush_since`, and persists the updated cursor for incremental
    /// export on the next call.
    ///
    /// The cold backend determines what "flush" means:
    /// - `NullStorage`: no-op (no cold storage configured)
    /// - `SqlNormalizedStorage`: no-op (dual-write keeps SQLite in sync)
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

// ── Cypher query — delegates to storage (DualStorage → cold) ─────────────

impl CypherCapable for DefaultBlackboard {
    fn query_plan(&self, plan: &serde_json::Value) -> Result<serde_json::Value, String> {
        self.storage.query_plan(plan)
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

    fn conclude_intent(
        &mut self,
        intent_id: &str,
        result: &serde_json::Value,
    ) -> Result<Fact, BlackboardError> {
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
    use crate::cypher;
    use nexus_model::{Blackboard, Fact, FihHash, Intent};

    #[test]
    fn test_storage_snapshot_roundtrip() {
        let mut bb = DefaultBlackboard::new();

        bb.submit_fact(&Fact {
            id: FihHash("f_snap_1".into()),
            origin: "snap-test".into(),
            content: serde_json::json!("snapshot data"),
            creator: "tester".into(),
        })
        .unwrap();
        bb.submit_fact(&Fact {
            id: FihHash("f_snap_2".into()),
            origin: "snap-test".into(),
            content: serde_json::json!("more data"),
            creator: "tester".into(),
        })
        .unwrap();
        bb.submit_intent(&Intent {
            id: FihHash("i_snap_1".into()),
            from_facts: vec!["f_snap_1".into()],
            description: "snapshot intent".into(),
            creator: "tester".into(),
            worker: None,
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            concluded_at: None,
        })
        .unwrap();

        let snapshot = bb.to_snapshot();
        let json = serde_json::to_vec(&snapshot).expect("serialise");
        let restored_snapshot: StorageSnapshot =
            serde_json::from_slice(&json).expect("deserialise");
        let mut restored = DefaultBlackboard::from_snapshot_inner(&restored_snapshot);

        let state = <DefaultBlackboard as Blackboard>::read_state(&restored);
        assert_eq!(state.facts.len(), 2);
        assert_eq!(state.intents.len(), 1);

        let plan = cypher::Plan::from_internal("MATCH (f:Fact) RETURN f").unwrap();
        let count = cypher::execute(&restored, &plan).unwrap().len();
        assert_eq!(count, 2, "Cypher works on restored snapshot");

        restored
            .claim_intent("i_snap_1", "agent-x")
            .expect("claim should work on restored bb");
    }
}
