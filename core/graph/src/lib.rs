// nexus-graph — GraphBlackboard: petgraph-backed FIH implementation.
//
// Depends on nexus-model for the Blackboard trait, Storage trait, and FIH primitives.
// GraphAccess trait is petgraph-specific and lives here.

pub mod cypher;
pub mod mock_gateway;

pub use nexus_model::{
    Blackboard, BlackboardError, BoardState, Fact, FihHash, Hint, Intent, NullStorage, Storage,
    StoredEvent,
};
use petgraph::graph::{EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Internal storage types ────────────────────────────────────────────────

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

// ── GraphAccess trait (internal — cypher executor) ───────────────────────

pub trait GraphAccess {
    fn node_indices(&self) -> Vec<NodeIndex>;
    fn edge_indices(&self) -> Vec<EdgeIndex>;
    fn node_weight(&self, idx: NodeIndex) -> Option<&NodeWeight>;
    fn edge_weight(&self, idx: EdgeIndex) -> Option<&EdgeWeight>;
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
    fn node_weight(&self, idx: NodeIndex) -> Option<&NodeWeight> {
        petgraph::Graph::node_weight(self, idx)
    }
    fn edge_weight(&self, idx: EdgeIndex) -> Option<&EdgeWeight> {
        petgraph::Graph::edge_weight(self, idx)
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

// ── GraphBlackboard (petgraph-backed, implements both traits) ────────────

pub struct GraphBlackboard {
    graph: petgraph::Graph<NodeWeight, EdgeWeight>,
    signals: Vec<Signal>,
    storage: Box<dyn Storage>,
    loading: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signal {
    pub agent_id: String,
    pub zone: String,
    pub payload: String,
    pub timestamp: u64,
    pub decay_rate: f64,
}

impl Default for GraphBlackboard {
    fn default() -> Self {
        Self {
            graph: petgraph::Graph::new(),
            signals: Vec::new(),
            storage: Box::new(NullStorage),
            loading: false,
        }
    }
}

impl GraphBlackboard {
    /// In-memory only (no persistence).
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach a custom storage backend. Loads past events if any.
    pub fn with_storage(mut self, storage: Box<dyn Storage>) -> Self {
        self.storage = storage;
        self.loading = true;
        let events = self.storage.load_events();
        for event in &events {
            self.replay_one(&event.event_type, &event.payload);
        }
        self.loading = false;
        self
    }

    fn log_fih(&self, event_type: &str, payload: &str) {
        if !self.loading {
            self.storage.log_fih(event_type, payload);
        }
    }

    fn replay_one(&mut self, event_type: &str, payload: &str) {
        match event_type {
            "submit_fact" => {
                if let Ok(fact) = serde_json::from_str::<Fact>(payload) {
                    Blackboard::submit_fact(self, &fact);
                }
            }
            "submit_hint" => {
                if let Ok(hint) = serde_json::from_str::<Hint>(payload) {
                    self.submit_hint(&hint);
                }
            }
            "submit_intent" => {
                if let Ok(intent) = serde_json::from_str::<Intent>(payload) {
                    self.submit_intent(&intent);
                }
            }
            "claim_intent" => {
                if let Ok(c) = serde_json::from_str::<ClaimPayload>(payload) {
                    let _ = self.claim_intent(&c.id, &c.agent);
                }
            }
            "heartbeat" => {
                if let Ok(c) = serde_json::from_str::<ClaimPayload>(payload) {
                    let _ = self.heartbeat(&c.id, &c.agent);
                }
            }
            "release_intent" => {
                if let Ok(c) = serde_json::from_str::<ClaimPayload>(payload) {
                    let _ = self.release_intent(&c.id, &c.agent);
                }
            }
            "conclude_intent" => {
                if let Ok(c) = serde_json::from_str::<ConcludePayload>(payload) {
                    let v: serde_json::Value = c.result.into();
                    let _ = self.conclude_intent(&c.id, &v);
                }
            }
            _ => {}
        }
    }
}

#[derive(Serialize, Deserialize)]
struct ClaimPayload {
    id: String,
    agent: String,
}

#[derive(Serialize, Deserialize)]
struct ConcludePayload {
    id: String,
    result: String,
}

// ── Blackboard impl ──────────────────────────────────────────────────────

/// Convenience: create a persistent GraphBlackboard backed by SQLite via nexus-table.
pub fn blackboard_with_sqlite(path: &str) -> Result<GraphBlackboard, String> {
    let store = nexus_table::SqliteStorage::open(path).map_err(|e| e.to_string())?;
    Ok(GraphBlackboard::new().with_storage(Box::new(store)))
}

impl Blackboard for GraphBlackboard {
    fn submit_fact(&mut self, fact: &Fact) -> FihHash {
        let payload = serde_json::to_string(fact).unwrap();
        self.log_fih("submit_fact", &payload);
        // Store as petgraph node
        let _idx = self.graph.add_node(NodeWeight {
            name: fact.id.0.clone(),
            label: "Fact".into(),
            properties: {
                let mut m = std::collections::HashMap::new();
                m.insert("origin".into(), fact.origin.clone().into());
                m.insert("content".into(), fact.content.clone());
                m.insert("creator".into(), fact.creator.clone().into());
                m
            },
        });
        fact.id.clone()
    }

    fn submit_hint(&mut self, hint: &Hint) {
        let payload = serde_json::to_string(hint).unwrap();
        self.log_fih("submit_hint", &payload);
        // Store as petgraph node
        self.graph.add_node(NodeWeight {
            name: hint.id.0.clone(),
            label: "Hint".into(),
            properties: {
                let mut m = std::collections::HashMap::new();
                m.insert("content".into(), hint.content.clone().into());
                m.insert("creator".into(), hint.creator.clone().into());
                m
            },
        });
    }

    fn submit_intent(&mut self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        // Validate from_facts exist in graph
        for fid in &intent.from_facts {
            let found = self
                .graph
                .node_indices()
                .any(|i| self.graph.node_weight(i).map_or(false, |w| w.name == *fid));
            if !found {
                return Err(BlackboardError::NotFound(format!("Fact {fid} not found")));
            }
        }
        let payload = serde_json::to_string(intent).unwrap();
        self.log_fih("submit_intent", &payload);
        let idx = self.graph.add_node(NodeWeight {
            name: intent.id.0.clone(),
            label: "Intent".into(),
            properties: {
                let mut m = std::collections::HashMap::new();
                m.insert("description".into(), intent.description.clone().into());
                m.insert("creator".into(), intent.creator.clone().into());
                m
            },
        });
        // Create edges from each source fact to this intent
        for fid in &intent.from_facts {
            if let Some(src) = self
                .graph
                .node_indices()
                .find(|i| self.graph.node_weight(*i).map_or(false, |w| w.name == *fid))
            {
                self.graph.add_edge(
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

    fn claim_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let payload = serde_json::to_string(&ClaimPayload {
            id: intent_id.into(),
            agent: agent.into(),
        })
        .unwrap();
        self.log_fih("claim_intent", &payload);
        // In-memory tracking: intent claims are managed via petgraph edge properties.
        // For now, this is a no-op for the in-memory graph.
        Ok(())
    }

    fn heartbeat(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let payload = serde_json::to_string(&ClaimPayload {
            id: intent_id.into(),
            agent: agent.into(),
        })
        .unwrap();
        self.log_fih("heartbeat", &payload);
        Ok(())
    }

    fn release_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let payload = serde_json::to_string(&ClaimPayload {
            id: intent_id.into(),
            agent: agent.into(),
        })
        .unwrap();
        self.log_fih("release_intent", &payload);
        Ok(())
    }

    fn conclude_intent(
        &mut self,
        intent_id: &str,
        result: &serde_json::Value,
    ) -> Result<Fact, BlackboardError> {
        let result_str = serde_json::to_string(result).unwrap();
        let payload = serde_json::to_string(&ConcludePayload {
            id: intent_id.into(),
            result: result_str.clone(),
        })
        .unwrap();
        self.log_fih("conclude_intent", &payload);

        // Create a new fact from the conclusion
        let new_fact = Fact {
            id: FihHash::new(&[intent_id, &result_str], "conclusion"),
            origin: format!("conclusion:{}", intent_id),
            content: result.clone(),
            creator: "system".into(),
        };
        let _ = self.submit_fact(&new_fact);
        Ok(new_fact)
    }

    fn read_state(&self) -> BoardState {
        let mut facts = Vec::new();
        let mut intents = Vec::new();
        let mut hints = Vec::new();

        for idx in self.graph.node_indices() {
            if let Some(w) = self.graph.node_weight(idx) {
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
                        intents.push(Intent {
                            id: FihHash(w.name.clone()),
                            from_facts: {
                                let mut src = Vec::new();
                                for e in self
                                    .graph
                                    .edges_directed(idx, petgraph::Direction::Incoming)
                                {
                                    if let Some(sn) = self.graph.node_weight(e.source()) {
                                        src.push(sn.name.clone());
                                    }
                                }
                                src
                            },
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
                            worker: None,
                            concluded_at: None,
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
