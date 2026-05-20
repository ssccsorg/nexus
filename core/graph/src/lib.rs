// nexus-graph — GraphBlackboard: petgraph-backed FIH implementation.
//
// Depends on nexus-model for the Blackboard trait and FIH primitives.
// Uses nexus-table::SqlBlackboard for normalized persistence.
// GraphAccess trait is petgraph-specific and lives here.

pub mod cypher;
pub mod mock_gateway;

pub use nexus_model::{
    Blackboard, BlackboardError, BoardState, Fact, FihHash, Hint, Intent,
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
    pub graph: petgraph::Graph<NodeWeight, EdgeWeight>,
    _signals: Vec<Signal>,
    claims: HashMap<String, String>,
    persist: Option<nexus_table::SqlBlackboard>,
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
            _signals: Vec::new(),
            claims: HashMap::new(),
            persist: None,
            loading: false,
        }
    }
}

impl GraphBlackboard {
    /// In-memory only (no persistence).
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach SqlBlackboard for normalized persistence.
    /// Loads existing facts/intents/hints from storage into the graph on init.
    pub fn with_persistence(mut self, db: nexus_table::SqlBlackboard) -> Self {
        self.persist = Some(db);
        self.loading = true;
        if let Some(ref db) = self.persist {
            let state = db.read_state();
            for fact in &state.facts {
                let _ = self.graph.add_node(NodeWeight {
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
            }
            for intent in &state.intents {
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
                // Restore edges from source facts
                for fid in &intent.from_facts {
                    if let Some(src) = self.graph.node_indices().find(|i| self.graph.node_weight(*i).is_some_and(|w| w.name == *fid)) {
                        self.graph.add_edge(src, idx, EdgeWeight {
                            rel_type: "drives".into(),
                            properties: HashMap::new(),
                        });
                    }
                }
                // Restore claim if intent has a worker
                if let Some(ref worker) = intent.worker {
                    self.claims.insert(intent.id.0.clone(), worker.clone());
                }
            }
            for hint in &state.hints {
                let _ = self.graph.add_node(NodeWeight {
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
        }
        self.loading = false;
        self
    }

    fn persist_apply<F>(&mut self, f: F)
    where
        F: FnOnce(&mut nexus_table::SqlBlackboard),
    {
        if !self.loading {
            if let Some(ref mut db) = self.persist {
                f(db);
            }
        }
    }
}

// ── Blackboard impl ──────────────────────────────────────────────────────

/// Convenience: create a persistent GraphBlackboard backed by SQLite via nexus-table.
pub fn blackboard_with_sqlite(path: &str) -> Result<GraphBlackboard, String> {
    let db = nexus_table::SqlBlackboard::open(path).map_err(|e| e.to_string())?;
    Ok(GraphBlackboard::new().with_persistence(db))
}

impl GraphAccess for GraphBlackboard {
    fn node_indices(&self) -> Vec<NodeIndex> {
        GraphAccess::node_indices(&self.graph)
    }
    fn edge_indices(&self) -> Vec<EdgeIndex> {
        GraphAccess::edge_indices(&self.graph)
    }
    fn node_weight(&self, idx: NodeIndex) -> Option<&NodeWeight> {
        GraphAccess::node_weight(&self.graph, idx)
    }
    fn edge_weight(&self, idx: EdgeIndex) -> Option<&EdgeWeight> {
        GraphAccess::edge_weight(&self.graph, idx)
    }
    fn edge_endpoints(&self, idx: EdgeIndex) -> Option<(NodeIndex, NodeIndex)> {
        GraphAccess::edge_endpoints(&self.graph, idx)
    }
    fn neighbors_undirected(&self, idx: NodeIndex) -> Vec<NodeIndex> {
        GraphAccess::neighbors_undirected(&self.graph, idx)
    }
    fn edges_directed(&self, idx: NodeIndex, outgoing: bool) -> Vec<EdgeIndex> {
        GraphAccess::edges_directed(&self.graph, idx, outgoing)
    }
    fn add_node(&mut self, weight: NodeWeight) -> NodeIndex {
        GraphAccess::add_node(&mut self.graph, weight)
    }
    fn add_edge(&mut self, from: NodeIndex, to: NodeIndex, weight: EdgeWeight) -> EdgeIndex {
        GraphAccess::add_edge(&mut self.graph, from, to, weight)
    }
}

impl Blackboard for GraphBlackboard {
    fn submit_fact(&mut self, fact: &Fact) -> Result<FihHash, BlackboardError> {
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
        self.persist_apply(|db| { let _ = db.submit_fact(fact); });
        Ok(fact.id.clone())
    }

    fn submit_hint(&mut self, hint: &Hint) -> Result<(), BlackboardError> {
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
        self.persist_apply(|db| { let _ = db.submit_hint(hint); });
        Ok(())
    }

    fn submit_intent(&mut self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        // Validate from_facts exist in graph
        for fid in &intent.from_facts {
            let found = self
                .graph
                .node_indices()
                .any(|i| self.graph.node_weight(i).is_some_and(|w| w.name == *fid));
            if !found {
                return Err(BlackboardError::NotFound(format!("Fact {fid} not found")));
            }
        }
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
                .find(|i| self.graph.node_weight(*i).is_some_and(|w| w.name == *fid))
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
        self.persist_apply(|db| { let _ = db.submit_intent(intent); });
        Ok(intent.id.clone())
    }

    fn claim_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        if let Some(current) = self.claims.get(intent_id) {
            return Err(BlackboardError::Conflict(format!(
                "Intent {} is already claimed by {}",
                intent_id, current
            )));
        }
        self.claims.insert(intent_id.to_string(), agent.to_string());
        self.persist_apply(|db| { let _ = db.claim_intent(intent_id, agent); });
        Ok(())
    }

    fn heartbeat(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        if let Some(current) = self.claims.get(intent_id) {
            if current != agent {
                return Err(BlackboardError::Conflict(format!(
                    "Intent {} is claimed by {}, not {}",
                    intent_id, current, agent
                )));
            }
        } else {
            self.claims.insert(intent_id.to_string(), agent.to_string());
        }
        self.persist_apply(|db| { let _ = db.heartbeat(intent_id, agent); });
        Ok(())
    }

    fn release_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        match self.claims.get(intent_id) {
            None => return Ok(()),
            Some(current) if current != agent => {
                return Err(BlackboardError::Conflict(format!(
                    "Intent {} is claimed by {}, not {}",
                    intent_id, current, agent
                )));
            }
            _ => {}
        }
        self.claims.remove(intent_id);
        self.persist_apply(|db| { let _ = db.release_intent(intent_id, agent); });
        Ok(())
    }

    fn conclude_intent(
        &mut self,
        intent_id: &str,
        result: &serde_json::Value,
    ) -> Result<Fact, BlackboardError> {
        let result_str = serde_json::to_string(result).unwrap();
        self.claims.remove(intent_id);
        let new_fact = Fact {
            id: FihHash::new(&[intent_id, &result_str], "conclusion"),
            origin: format!("conclusion:{}", intent_id),
            content: result.clone(),
            creator: "system".into(),
        };
        let _ = self.submit_fact(&new_fact);
        self.persist_apply(|db| { let _ = db.conclude_intent(intent_id, result); });
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
                            to_fact_id: None,
                            last_heartbeat_at: None,
                            created_at: None,
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
