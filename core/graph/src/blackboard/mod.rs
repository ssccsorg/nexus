// nexus-graph — DefaultBlackboard: the single blackboard struct.
//
// Part of the graph runtime. Combines a hot petgraph (for low-latency
// access and Cypher queries) with a cold storage backend for durability.
// Storage is swappable via DualStorage.

use nexus_model::{
    Blackboard, BlackboardError, BoardState, ColdStorage, DualStorage, Fact, FactCapable, FihHash,
    Hint, HintCapable, Intent, IntentCapable, NullStorage, StorageRead,
};
use nexus_storage_petgraph::{EdgeWeight, GraphRead, GraphWrite, NodeWeight, PetgraphStorage};
use petgraph::graph::{EdgeIndex, NodeIndex};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

/// Tracks intent claims — which agent has claimed which intent.
///
/// Acts as a local cache in front of the storage layer's claim tracking.
/// The storage layer is the source of truth; this cache provides fast
/// conflict detection without a storage round-trip.
struct ClaimsTracker {
    inner: HashMap<String, String>,
}

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
struct ClaimGuard<'a> {
    claims: &'a Mutex<ClaimsTracker>,
    intent_id: String,
    agent: String,
    committed: bool,
}

impl<'a> ClaimGuard<'a> {
    fn new(claims: &'a Mutex<ClaimsTracker>, intent_id: String, agent: String) -> Self {
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
            if let Ok(mut claims) = self.claims.lock() {
                let _ = claims.release(&self.intent_id, &self.agent);
            }
        }
    }
}

impl ClaimsTracker {
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
pub struct DefaultBlackboard {
    storage: DualStorage,
    hot_graph: Arc<RwLock<petgraph::Graph<NodeWeight, EdgeWeight>>>,
    claims: Mutex<ClaimsTracker>,
    project_id: String,
}

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
            claims: Mutex::new(ClaimsTracker::new()),
            project_id: "default".into(),
        }
    }

    pub fn with_storage(hot: PetgraphStorage, cold: Box<dyn ColdStorage>) -> Self {
        let hot_graph = Arc::clone(&hot.graph);
        let project_id = hot.project_id.clone();
        let storage = DualStorage::new(Box::new(hot), cold);

        Self {
            storage,
            hot_graph,
            claims: Mutex::new(ClaimsTracker::new()),
            project_id,
        }
    }

    pub fn with_graph<R>(
        &self,
        f: impl FnOnce(&petgraph::Graph<NodeWeight, EdgeWeight>) -> R,
    ) -> R {
        let g = self.hot_graph.read().unwrap();
        f(&g)
    }

    pub fn flush(&self) -> Result<(), String> {
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
}

impl Default for DefaultBlackboard {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphRead for DefaultBlackboard {
    fn node_indices(&self) -> Vec<NodeIndex> {
        let g = self.hot_graph.read().unwrap();
        g.node_indices()
    }

    fn edge_indices(&self) -> Vec<EdgeIndex> {
        let g = self.hot_graph.read().unwrap();
        g.edge_indices()
    }

    fn node_weight(&self, idx: NodeIndex) -> Option<NodeWeight> {
        let g = self.hot_graph.read().unwrap();
        g.node_weight(idx)
    }

    fn edge_weight(&self, idx: EdgeIndex) -> Option<EdgeWeight> {
        let g = self.hot_graph.read().unwrap();
        g.edge_weight(idx)
    }

    fn edge_endpoints(&self, idx: EdgeIndex) -> Option<(NodeIndex, NodeIndex)> {
        let g = self.hot_graph.read().unwrap();
        g.edge_endpoints(idx)
    }

    fn neighbors_undirected(&self, idx: NodeIndex) -> Vec<NodeIndex> {
        let g = self.hot_graph.read().unwrap();
        g.neighbors_undirected(idx)
    }

    fn edges_directed(&self, idx: NodeIndex, outgoing: bool) -> Vec<EdgeIndex> {
        let g = self.hot_graph.read().unwrap();
        g.edges_directed(idx, outgoing)
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
        let mut claims = self.claims.lock().unwrap();
        claims.try_claim(intent_id, agent)?;
        let mut guard = ClaimGuard::new(&self.claims, intent_id.to_string(), agent.to_string());
        self.storage.claim_intent(intent_id, agent)?;
        guard.commit();
        Ok(())
    }

    fn heartbeat(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let claims = self.claims.lock().unwrap();
        claims.verify_owner(intent_id, agent)?;
        self.storage.heartbeat(intent_id, agent)
    }

    fn release_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let mut claims = self.claims.lock().unwrap();
        claims.release(intent_id, agent)?;
        self.storage.release_intent(intent_id, agent)
    }

    fn conclude_intent(
        &mut self,
        intent_id: &str,
        result: &serde_json::Value,
    ) -> Result<Fact, BlackboardError> {
        let mut claims = self.claims.lock().unwrap();
        claims.remove(intent_id);
        self.storage.conclude_intent(intent_id, result)
    }

    fn read_state(&self) -> BoardState {
        self.storage.read_state()
    }
}
