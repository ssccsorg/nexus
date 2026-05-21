// nexus-graph — DefaultBlackboard: the single blackboard struct.
//
// Part of the graph runtime. Combines a hot petgraph (for low-latency
// access and Cypher queries) with a cold storage backend for durability.
// Storage is swappable via DualStorage.

use nexus_model::{
    Blackboard, BlackboardError, BoardState, ColdStorage, DualStorage, Fact, FactCapable, FihHash,
    Hint, HintCapable, Intent, IntentCapable, NullStorage, StorageRead,
};
use nexus_storage_petgraph::{EdgeWeight, GraphAccess, NodeWeight, PetgraphStorage};
use petgraph::graph::{EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The single Blackboard struct. Combines a hot petgraph (for low-latency
/// access and Cypher queries) with a cold storage backend for durability.
pub struct DefaultBlackboard {
    storage: DualStorage,
    hot_graph: Arc<Mutex<petgraph::Graph<NodeWeight, EdgeWeight>>>,
    claims: Mutex<HashMap<String, String>>,
    project_id: String,
}

impl DefaultBlackboard {
    pub fn new() -> Self {
        let graph = Arc::new(Mutex::new(petgraph::Graph::new()));
        let hot = Box::new(PetgraphStorage::with_shared_graph(
            Arc::clone(&graph),
            "default",
        ));
        let cold = Box::new(NullStorage);
        let storage = DualStorage::new(hot, cold);

        Self {
            storage,
            hot_graph: graph,
            claims: Mutex::new(HashMap::new()),
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
            claims: Mutex::new(HashMap::new()),
            project_id,
        }
    }

    pub fn with_graph<R>(
        &self,
        f: impl FnOnce(&petgraph::Graph<NodeWeight, EdgeWeight>) -> R,
    ) -> R {
        let g = self.hot_graph.lock().unwrap();
        f(&g)
    }

    pub fn flush(&self) -> Result<(), String> {
        Ok(())
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }
}

impl Default for DefaultBlackboard {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphAccess for DefaultBlackboard {
    fn node_indices(&self) -> Vec<NodeIndex> {
        let g = self.hot_graph.lock().unwrap();
        g.node_indices().collect()
    }

    fn edge_indices(&self) -> Vec<EdgeIndex> {
        let g = self.hot_graph.lock().unwrap();
        g.edge_indices().collect()
    }

    fn node_weight(&self, idx: NodeIndex) -> Option<NodeWeight> {
        let g = self.hot_graph.lock().unwrap();
        g.node_weight(idx).cloned()
    }

    fn edge_weight(&self, idx: EdgeIndex) -> Option<EdgeWeight> {
        let g = self.hot_graph.lock().unwrap();
        g.edge_weight(idx).cloned()
    }

    fn edge_endpoints(&self, idx: EdgeIndex) -> Option<(NodeIndex, NodeIndex)> {
        let g = self.hot_graph.lock().unwrap();
        g.edge_endpoints(idx)
    }

    fn neighbors_undirected(&self, idx: NodeIndex) -> Vec<NodeIndex> {
        let g = self.hot_graph.lock().unwrap();
        g.neighbors_undirected(idx).collect()
    }

    fn edges_directed(&self, idx: NodeIndex, outgoing: bool) -> Vec<EdgeIndex> {
        let g = self.hot_graph.lock().unwrap();
        let dir = if outgoing {
            petgraph::Direction::Outgoing
        } else {
            petgraph::Direction::Incoming
        };
        g.edges_directed(idx, dir).map(|e| e.id()).collect()
    }

    fn add_node(&mut self, weight: NodeWeight) -> NodeIndex {
        let mut g = self.hot_graph.lock().unwrap();
        g.add_node(weight)
    }

    fn add_edge(&mut self, from: NodeIndex, to: NodeIndex, weight: EdgeWeight) -> EdgeIndex {
        let mut g = self.hot_graph.lock().unwrap();
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
        if let Some(current) = claims.get(intent_id) {
            return Err(BlackboardError::Conflict(format!(
                "Intent {intent_id} already claimed by {current}"
            )));
        }
        self.storage.claim_intent(intent_id, agent)?;
        claims.insert(intent_id.to_string(), agent.to_string());
        Ok(())
    }

    fn heartbeat(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let mut claims = self.claims.lock().unwrap();
        match claims.get(intent_id) {
            Some(current) if current != agent => {
                return Err(BlackboardError::Conflict(format!(
                    "Intent {intent_id} is claimed by {current}, not {agent}"
                )));
            }
            _ => {
                claims.insert(intent_id.to_string(), agent.to_string());
            }
        }
        self.storage.heartbeat(intent_id, agent)
    }

    fn release_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let mut claims = self.claims.lock().unwrap();
        match claims.get(intent_id) {
            None => return self.storage.release_intent(intent_id, agent),
            Some(current) if current != agent => {
                return Err(BlackboardError::Conflict(format!(
                    "Intent {intent_id} is claimed by {current}, not {agent}"
                )));
            }
            _ => {
                claims.remove(intent_id);
            }
        }
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
