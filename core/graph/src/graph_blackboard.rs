// nexus-graph — GraphBlackboard: the single Blackboard struct.
//
// Combines a hot petgraph (for low-latency access and Cypher queries)
// with a cold storage backend for durability. Storage is swappable
// via DualStorage. Claims tracking is local to this struct.

use crate::graph_access::GraphAccess;
use crate::petgraph_storage::PetgraphStorage;
use crate::weight::{EdgeWeight, NodeWeight};
use nexus_model::{
    Blackboard, BlackboardError, BoardState, ColdStorage, DualStorage, Fact, FactCapable, FihHash,
    Hint, HintCapable, Intent, IntentCapable, NullStorage, StorageRead,
};
use petgraph::graph::{EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The single Blackboard struct. Combines a hot petgraph (for low-latency
/// access and Cypher queries) with a cold storage backend for durability.
pub struct GraphBlackboard {
    storage: DualStorage,
    hot_graph: Arc<Mutex<petgraph::Graph<NodeWeight, EdgeWeight>>>,
    claims: Mutex<HashMap<String, String>>,
    project_id: String,
}

impl GraphBlackboard {
    /// Create an in-memory only Blackboard (hot=PetgraphStorage, cold=NullStorage).
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

    /// Create a Blackboard with custom hot and cold storage backends.
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

    /// Access the petgraph directly (for Cypher executor).
    pub fn with_graph<R>(
        &self,
        f: impl FnOnce(&petgraph::Graph<NodeWeight, EdgeWeight>) -> R,
    ) -> R {
        let g = self.hot_graph.lock().unwrap();
        f(&g)
    }

    /// Flush pending writes to cold storage.
    /// TODO(#51): wire to FlushCapable once DualStorage delegates it.
    pub fn flush(&self) -> Result<(), String> {
        Ok(())
    }

    /// Return the project ID for this Blackboard.
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
}

impl Default for GraphBlackboard {
    fn default() -> Self {
        Self::new()
    }
}

impl GraphAccess for GraphBlackboard {
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

impl Blackboard for GraphBlackboard {
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
            None => {
                return self.storage.release_intent(intent_id, agent);
            }
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
