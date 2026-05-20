// nexus-graph — GraphBlackboard: petgraph-backed FIH Blackboard.
//
// Architecture
// ============
//
//   GraphBlackboard (implements Blackboard + GraphAccess)
//     ├── storage: DualStorage (hot + cold)
//     │     ├── hot: PetgraphStorage (Arc<Mutex<petgraph::Graph>>)
//     │     └── cold: ColdStorage (NullStorage | any external impl)
//     ├── hot_graph: Arc<Mutex<petgraph::Graph>> (shared with PetgraphStorage)
//     ├── claims: Mutex<HashMap<IntentId, Agent>>
//     └── project_id: String
//
// `GraphBlackboard` is the **single** Blackboard struct. Storage is swappable
// via DualStorage. The petgraph hot store is shared through an Arc so that
// both GraphBlackboard (for GraphAccess / Cypher queries) and PetgraphStorage
// (for Storage operations) access the same in-memory graph.
//
// Public API re-exports from nexus-model (Storage, Blackboard traits, FIH types).

pub mod cypher;
pub mod mock_gateway;

pub use nexus_model::{
    Blackboard, BlackboardError, BoardState, ColdStorage, DualStorage, Fact, FihHash, Hint,
    HotStorage, Intent, NullStorage, Storage,
};
use petgraph::graph::{EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// ── Node and edge weight types ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeWeight {
    pub name: String,
    pub label: String,
    pub properties: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeWeight {
    pub rel_type: String,
    pub properties: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub fields: HashMap<String, serde_json::Value>,
}

// ── GraphAccess trait ──────────────────────────────────────────────────

/// Access interface for petgraph queries (Cypher executor).
///
/// All methods return owned values so the trait can be implemented for
/// types behind a Mutex (e.g. GraphBlackboard).
pub trait GraphAccess {
    fn node_indices(&self) -> Vec<NodeIndex>;
    fn edge_indices(&self) -> Vec<EdgeIndex>;
    fn node_weight(&self, idx: NodeIndex) -> Option<NodeWeight>;
    fn edge_weight(&self, idx: EdgeIndex) -> Option<EdgeWeight>;
    fn edge_endpoints(&self, idx: EdgeIndex) -> Option<(NodeIndex, NodeIndex)>;
    fn neighbors_undirected(&self, idx: NodeIndex) -> Vec<NodeIndex>;
    fn edges_directed(&self, idx: NodeIndex, outgoing: bool) -> Vec<EdgeIndex>;
    fn add_node(&mut self, weight: NodeWeight) -> NodeIndex;
    fn add_edge(&mut self, from: NodeIndex, to: NodeIndex, weight: EdgeWeight) -> EdgeIndex;
}

impl GraphAccess for petgraph::Graph<NodeWeight, EdgeWeight> {
    fn node_indices(&self) -> Vec<NodeIndex> {
        self.node_indices().collect()
    }
    fn edge_indices(&self) -> Vec<EdgeIndex> {
        self.edge_indices().collect()
    }
    fn node_weight(&self, idx: NodeIndex) -> Option<NodeWeight> {
        petgraph::Graph::node_weight(self, idx).cloned()
    }
    fn edge_weight(&self, idx: EdgeIndex) -> Option<EdgeWeight> {
        petgraph::Graph::edge_weight(self, idx).cloned()
    }
    fn edge_endpoints(&self, idx: EdgeIndex) -> Option<(NodeIndex, NodeIndex)> {
        self.edge_endpoints(idx)
    }
    fn neighbors_undirected(&self, idx: NodeIndex) -> Vec<NodeIndex> {
        self.neighbors_undirected(idx).collect()
    }
    fn edges_directed(&self, idx: NodeIndex, outgoing: bool) -> Vec<EdgeIndex> {
        let dir = if outgoing {
            petgraph::Direction::Outgoing
        } else {
            petgraph::Direction::Incoming
        };
        self.edges_directed(idx, dir).map(|e| e.id()).collect()
    }
    fn add_node(&mut self, weight: NodeWeight) -> NodeIndex {
        self.add_node(weight)
    }
    fn add_edge(&mut self, from: NodeIndex, to: NodeIndex, weight: EdgeWeight) -> EdgeIndex {
        self.add_edge(from, to, weight)
    }
}

// ── PetgraphStorage — HotStorage implementation ────────────────────────

/// In-memory petgraph-backed storage. Thread-safe via internal Mutex.
///
/// The underlying `petgraph::Graph` is shared through an `Arc<Mutex<...>>`
/// so that `GraphBlackboard` can access the same graph for Cypher queries
/// while `PetgraphStorage` handles FIH persistence.
pub struct PetgraphStorage {
    graph: Arc<Mutex<petgraph::Graph<NodeWeight, EdgeWeight>>>,
    project_id: String,
}

impl PetgraphStorage {
    /// Creates a new empty PetgraphStorage with project_id "default".
    pub fn new() -> Self {
        Self::with_project_id("default")
    }

    /// Creates a new empty PetgraphStorage with a specific project_id.
    pub fn with_project_id(project_id: &str) -> Self {
        Self {
            graph: Arc::new(Mutex::new(petgraph::Graph::new())),
            project_id: project_id.to_string(),
        }
    }

    /// Creates a new PetgraphStorage sharing the given graph Arc.
    fn with_shared_graph(
        graph: Arc<Mutex<petgraph::Graph<NodeWeight, EdgeWeight>>>,
        project_id: &str,
    ) -> Self {
        Self {
            graph,
            project_id: project_id.to_string(),
        }
    }

    /// Access the graph immutably (for GraphAccess delegation).
    pub fn with_graph<R>(
        &self,
        f: impl FnOnce(&petgraph::Graph<NodeWeight, EdgeWeight>) -> R,
    ) -> R {
        let g = self.graph.lock().unwrap();
        f(&g)
    }

    /// Access the graph mutably (for Storage operations).
    pub fn with_graph_mut<R>(
        &self,
        f: impl FnOnce(&mut petgraph::Graph<NodeWeight, EdgeWeight>) -> R,
    ) -> R {
        let mut g = self.graph.lock().unwrap();
        f(&mut g)
    }

    /// Flush is a no-op for in-memory storage.
    pub fn flush(&self) -> Result<(), String> {
        Ok(())
    }
}

impl Default for PetgraphStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl Storage for PetgraphStorage {
    fn project_id(&self) -> &str {
        &self.project_id
    }

    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        let mut g = self.graph.lock().unwrap();
        g.add_node(NodeWeight {
            name: fact.id.0.clone(),
            label: "Fact".into(),
            properties: {
                let mut m = HashMap::new();
                m.insert("origin".into(), fact.origin.clone().into());
                m.insert("content".into(), fact.content.clone());
                m.insert("creator".into(), fact.creator.clone().into());
                m
            },
        });
        Ok(fact.id.clone())
    }

    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        let mut g = self.graph.lock().unwrap();
        g.add_node(NodeWeight {
            name: hint.id.0.clone(),
            label: "Hint".into(),
            properties: {
                let mut m = HashMap::new();
                m.insert("content".into(), hint.content.clone().into());
                m.insert("creator".into(), hint.creator.clone().into());
                m
            },
        });
        Ok(())
    }

    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        let mut g = self.graph.lock().unwrap();

        // Validate from_facts exist in graph
        for fid in &intent.from_facts {
            let found = g
                .node_indices()
                .any(|i| g.node_weight(i).is_some_and(|w| w.name == *fid));
            if !found {
                return Err(BlackboardError::NotFound(format!("Fact {fid} not found")));
            }
        }

        let idx = g.add_node(NodeWeight {
            name: intent.id.0.clone(),
            label: "Intent".into(),
            properties: {
                let mut m = HashMap::new();
                m.insert("description".into(), intent.description.clone().into());
                m.insert("creator".into(), intent.creator.clone().into());
                m
            },
        });

        // Create edges from each source fact to this intent
        for fid in &intent.from_facts {
            if let Some(src) = g
                .node_indices()
                .find(|i| g.node_weight(*i).is_some_and(|w| w.name == *fid))
            {
                g.add_edge(
                    src,
                    idx,
                    EdgeWeight {
                        rel_type: "drives".into(),
                        properties: HashMap::new(),
                    },
                );
            }
        }

        Ok(intent.id.clone())
    }

    fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let mut g = self.graph.lock().unwrap();
        for idx in g.node_indices() {
            if let Some(w) = g.node_weight_mut(idx) {
                if w.name == intent_id && w.label == "Intent" {
                    // Check if already concluded
                    if w.properties
                        .get("concluded")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        return Err(BlackboardError::NotFound(format!(
                            "Intent {intent_id} already concluded"
                        )));
                    }
                    // Check if already claimed
                    if let Some(serde_json::Value::String(current)) = w.properties.get("worker") {
                        return Err(BlackboardError::Conflict(format!(
                            "Intent {intent_id} already claimed by {current}"
                        )));
                    }
                    w.properties
                        .insert("worker".into(), agent.to_string().into());
                    return Ok(());
                }
            }
        }
        Err(BlackboardError::NotFound(format!(
            "Intent {intent_id} not found"
        )))
    }

    fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let mut g = self.graph.lock().unwrap();
        for idx in g.node_indices() {
            if let Some(w) = g.node_weight_mut(idx) {
                if w.name == intent_id && w.label == "Intent" {
                    // Check if already concluded
                    if w.properties
                        .get("concluded")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        return Err(BlackboardError::NotFound(format!(
                            "Intent {intent_id} already concluded"
                        )));
                    }
                    let current = w.properties.get("worker");
                    match current {
                        Some(serde_json::Value::String(existing)) if existing != agent => {
                            return Err(BlackboardError::Conflict(format!(
                                "Intent {intent_id} is claimed by {existing}, not {agent}"
                            )));
                        }
                        _ => {
                            w.properties
                                .insert("worker".into(), agent.to_string().into());
                            return Ok(());
                        }
                    }
                }
            }
        }
        Err(BlackboardError::NotFound(format!(
            "Intent {intent_id} not found"
        )))
    }

    fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let mut g = self.graph.lock().unwrap();
        for idx in g.node_indices() {
            if let Some(w) = g.node_weight_mut(idx) {
                if w.name == intent_id && w.label == "Intent" {
                    // Check if already concluded
                    if w.properties
                        .get("concluded")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        return Err(BlackboardError::NotFound(format!(
                            "Intent {intent_id} already concluded"
                        )));
                    }
                    let current = w.properties.get("worker").cloned();
                    match current {
                        None => {
                            // Already unclaimed — no-op
                            return Ok(());
                        }
                        Some(serde_json::Value::String(existing)) if existing != agent => {
                            return Err(BlackboardError::Forbidden(format!(
                                "Intent {intent_id} claimed by {existing}"
                            )));
                        }
                        _ => {
                            w.properties.remove("worker");
                            return Ok(());
                        }
                    }
                }
            }
        }
        Err(BlackboardError::NotFound(format!(
            "Intent {intent_id} not found"
        )))
    }

    fn conclude_intent(
        &self,
        intent_id: &str,
        result: &serde_json::Value,
    ) -> Result<Fact, BlackboardError> {
        let mut g = self.graph.lock().unwrap();

        // Find the intent node
        let intent_idx = g
            .node_indices()
            .find(|i| {
                g.node_weight(*i)
                    .is_some_and(|w| w.name == intent_id && w.label == "Intent")
            })
            .ok_or_else(|| BlackboardError::NotFound(format!("Intent {intent_id} not found")))?;

        let worker = g
            .node_weight(intent_idx)
            .and_then(|w| w.properties.get("worker"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        // Mark intent as concluded
        if let Some(w) = g.node_weight_mut(intent_idx) {
            w.properties
                .insert("concluded".into(), serde_json::Value::Bool(true));
            w.properties.remove("worker");
        }

        // Create conclusion fact node
        let new_fact_id = format!("f_concl_{}", intent_id);
        let new_fact = Fact {
            id: FihHash(new_fact_id.clone()),
            origin: format!("conclusion:{}", intent_id),
            content: result.clone(),
            creator: worker,
        };

        let fact_idx = g.add_node(NodeWeight {
            name: new_fact_id,
            label: "Fact".into(),
            properties: {
                let mut m = HashMap::new();
                m.insert("origin".into(), new_fact.origin.clone().into());
                m.insert("content".into(), new_fact.content.clone());
                m.insert("creator".into(), new_fact.creator.clone().into());
                m
            },
        });

        // Edge from conclusion fact back to intent
        g.add_edge(
            intent_idx,
            fact_idx,
            EdgeWeight {
                rel_type: "concludes".into(),
                properties: HashMap::new(),
            },
        );

        Ok(new_fact)
    }

    fn read_state(&self) -> BoardState {
        let g = self.graph.lock().unwrap();
        let mut facts = Vec::new();
        let mut intents = Vec::new();
        let mut hints = Vec::new();

        for idx in g.node_indices() {
            if let Some(w) = g.node_weight(idx) {
                match w.label.as_str() {
                    "Fact" => {
                        facts.push(Fact {
                            id: FihHash(w.name.clone()),
                            origin: w
                                .properties
                                .get("origin")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .into(),
                            content: w
                                .properties
                                .get("content")
                                .cloned()
                                .unwrap_or(serde_json::Value::Null),
                            creator: w
                                .properties
                                .get("creator")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .into(),
                        });
                    }
                    "Intent" => {
                        let from_facts: Vec<String> = g
                            .edges_directed(idx, petgraph::Direction::Incoming)
                            .filter_map(|e| {
                                let sn = g.node_weight(e.source())?;
                                Some(sn.name.clone())
                            })
                            .collect();

                        intents.push(Intent {
                            id: FihHash(w.name.clone()),
                            from_facts,
                            description: w
                                .properties
                                .get("description")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .into(),
                            creator: w
                                .properties
                                .get("creator")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .into(),
                            worker: w
                                .properties
                                .get("worker")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                            to_fact_id: {
                                // Find conclusion fact node edge
                                g.edges_directed(idx, petgraph::Direction::Outgoing)
                                    .find(|e| {
                                        g.node_weight(e.target()).is_some_and(|n| {
                                            n.label == "Fact" && n.name.starts_with("f_concl_")
                                        })
                                    })
                                    .and_then(|e| g.node_weight(e.target()).map(|n| n.name.clone()))
                            },
                            last_heartbeat_at: None,
                            created_at: None,
                            concluded_at: if w
                                .properties
                                .get("concluded")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false)
                            {
                                Some("yes".into())
                            } else {
                                None
                            },
                        });
                    }
                    "Hint" => {
                        hints.push(Hint {
                            id: FihHash(w.name.clone()),
                            content: w
                                .properties
                                .get("content")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .into(),
                            creator: w
                                .properties
                                .get("creator")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .into(),
                        });
                    }
                    _ => {}
                }
            }
        }

        BoardState {
            facts,
            intents,
            hints,
        }
    }
}

impl HotStorage for PetgraphStorage {}

// ── GraphBlackboard — the single Blackboard ────────────────────────────

/// The single Blackboard struct. Combines a hot petgraph (for low-latency
/// access and Cypher queries) with a cold storage backend for durability.
///
/// # Constructors
///
/// - [`GraphBlackboard::new()`] — in-memory only (hot=PetgraphStorage, cold=NullStorage).
/// - [`GraphBlackboard::with_storage(hot, cold)`] — custom hot + cold pair.
pub struct GraphBlackboard {
    /// DualStorage wrapping hot (PetgraphStorage) + cold (ColdStorage).
    storage: DualStorage,
    /// Shared reference to the petgraph (for GraphAccess / Cypher).
    hot_graph: Arc<Mutex<petgraph::Graph<NodeWeight, EdgeWeight>>>,
    /// Claims tracking: IntentId → Agent.
    claims: Mutex<HashMap<String, String>>,
    /// Project scope.
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
    /// The hot storage must be a `PetgraphStorage` whose graph is shared
    /// with this Blackboard for GraphAccess.
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

    /// Access the petgraph directly (for Cypher executor and other
    /// low-level graph traversal).
    pub fn with_graph<R>(
        &self,
        f: impl FnOnce(&petgraph::Graph<NodeWeight, EdgeWeight>) -> R,
    ) -> R {
        let g = self.hot_graph.lock().unwrap();
        f(&g)
    }

    /// Flush pending writes to cold storage.
    /// Currently a no-op since every write is dual-written.
    pub fn flush(&self) -> Result<(), String> {
        self.storage.flush()
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
