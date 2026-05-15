// nexus-graph — GraphBlackboard: petgraph-backed FIH implementation.
//
// Depends on nexus-api for the Blackboard trait and FIH primitives.
// GraphAccess trait is petgraph-specific and lives here.

pub mod cypher;
pub mod storage;

pub use nexus_api::{Blackboard, BlackboardError, BoardState, Fact, FihHash, Hint, Intent};
pub use storage::{Storage, NullStorage, StoredEvent};
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

impl GraphBlackboard {
    /// In-memory only (no persistence).
    pub fn new() -> Self {
        Self {
            graph: petgraph::Graph::new(),
            signals: Vec::new(),
            storage: Box::new(NullStorage),
            loading: false,
        }
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
                    let _ = self.submit_intent(&intent);
                }
            }
            "claim_intent" => {
                if let Ok(c) = serde_json::from_str::<(String, String)>(payload) {
                    let _ = Blackboard::claim_intent(self, &c.0, &c.1);
                }
            }
            "release_intent" => {
                if let Ok(c) = serde_json::from_str::<(String, String)>(payload) {
                    let _ = self.release_intent(&c.0, &c.1);
                }
            }
            "conclude_intent" => {
                if let Ok(c) = serde_json::from_str::<(String, String)>(payload) {
                    let _ = self.conclude_intent(&c.0, &c.1);
                }
            }
            _ => {}
        }
    }

    fn find_node_by_name(&self, name: &str) -> Option<NodeIndex> {
        self.graph
            .node_indices()
            .find(|&idx| self.graph.node_weight(idx).is_some_and(|w| w.name == name))
    }

    fn add_fact(&mut self, fact: &Fact) -> NodeIndex {
        let mut props = HashMap::new();
        props.insert(
            "origin".into(),
            serde_json::Value::String(fact.origin.clone()),
        );
        props.insert(
            "content".into(),
            serde_json::Value::String(fact.content.clone()),
        );
        props.insert(
            "creator".into(),
            serde_json::Value::String(fact.creator.clone()),
        );
        self.graph.add_node(NodeWeight {
            name: fact.id.0.clone(),
            label: "Fact".into(),
            properties: props,
        })
    }

    fn add_intent(&mut self, intent: &Intent) -> NodeIndex {
        let mut props = HashMap::new();
        props.insert(
            "from_facts".into(),
            serde_json::Value::String(intent.from_facts.join(",")),
        );
        props.insert(
            "description".into(),
            serde_json::Value::String(intent.description.clone()),
        );
        props.insert(
            "creator".into(),
            serde_json::Value::String(intent.creator.clone()),
        );
        if let Some(ref w) = intent.worker {
            props.insert("worker".into(), serde_json::Value::String(w.clone()));
        }
        let ni = self.graph.add_node(NodeWeight {
            name: intent.id.0.clone(),
            label: "Intent".into(),
            properties: props,
        });
        for fact_id in &intent.from_facts {
            if let Some(fact_idx) = self.find_node_by_name(fact_id) {
                self.graph.add_edge(
                    ni,
                    fact_idx,
                    EdgeWeight {
                        rel_type: "GROUNDED_IN".into(),
                        properties: HashMap::new(),
                    },
                );
            }
        }
        ni
    }

    fn read_fact(&self, idx: NodeIndex) -> Option<Fact> {
        let w = self.graph.node_weight(idx)?;
        if w.label != "Fact" {
            return None;
        }
        Some(Fact {
            id: FihHash(w.name.clone()),
            origin: w.properties.get("origin")?.as_str()?.into(),
            content: w.properties.get("content")?.as_str()?.into(),
            creator: w.properties.get("creator")?.as_str()?.into(),
        })
    }

    fn read_intent(&self, idx: NodeIndex) -> Option<Intent> {
        let w = self.graph.node_weight(idx)?;
        if w.label != "Intent" {
            return None;
        }
        Some(Intent {
            id: FihHash(w.name.clone()),
            from_facts: w
                .properties
                .get("from_facts")
                .and_then(|v| v.as_str())
                .map(|s| s.split(',').map(|s| s.trim().into()).collect())
                .unwrap_or_default(),
            description: w.properties.get("description")?.as_str()?.into(),
            creator: w.properties.get("creator")?.as_str()?.into(),
            worker: w
                .properties
                .get("worker")
                .and_then(|v| v.as_str())
                .map(|s| s.into()),
            concluded_at: w
                .properties
                .get("concluded_at")
                .and_then(|v| v.as_str())
                .map(|s| s.into()),
        })
    }

    fn resolve_intent_name(&self, name_or_id: &str) -> Option<NodeIndex> {
        self.graph.node_indices().find(|&idx| {
            if let Some(w) = self.graph.node_weight(idx) {
                w.label == "Intent" && (w.name == name_or_id || w.name.ends_with(name_or_id))
            } else {
                false
            }
        })
    }

    #[allow(dead_code)]
    fn deposit_signal_internal(&mut self, signal: Signal) {
        self.signals.push(signal);
    }

    #[allow(dead_code)]
    fn read_signals_in_zone(&self, zone: &str) -> Vec<&Signal> {
        self.signals.iter().filter(|s| s.zone == zone).collect()
    }
}

impl Blackboard for GraphBlackboard {
    fn submit_fact(&mut self, fact: &Fact) -> FihHash {
        self.add_fact(fact);
        self.log_fih("submit_fact", &serde_json::to_string(fact).unwrap_or_default());
        fact.id.clone()
    }

    fn submit_hint(&mut self, hint: &Hint) {
        let mut props = HashMap::new();
        props.insert(
            "content".into(),
            serde_json::Value::String(hint.content.clone()),
        );
        props.insert(
            "creator".into(),
            serde_json::Value::String(hint.creator.clone()),
        );
        self.graph.add_node(NodeWeight {
            name: hint.id.0.clone(),
            label: "Hint".into(),
            properties: props,
        });
        self.log_fih("submit_hint", &serde_json::to_string(hint).unwrap_or_default());
    }

    fn submit_intent(&mut self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        if intent.from_facts.is_empty() {
            return Err(BlackboardError::Forbidden(
                "Intent must be grounded in at least one Fact".into(),
            ));
        }
        self.add_intent(intent);
        self.log_fih("submit_intent", &serde_json::to_string(intent).unwrap_or_default());
        Ok(intent.id.clone())
    }

    fn claim_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let idx = self
            .resolve_intent_name(intent_id)
            .ok_or_else(|| BlackboardError::NotFound(format!("Intent: {intent_id}")))?;
        let w = self.graph.node_weight(idx).unwrap();
        if w.properties.contains_key("concluded_at") {
            return Err(BlackboardError::Forbidden("Already concluded".into()));
        }
        if let Some(existing) = w.properties.get("worker").and_then(|v| v.as_str())
            && existing != agent
        {
            return Err(BlackboardError::Conflict(format!("Claimed by {existing}")));
        }
        let mut w = w.clone();
        w.properties
            .insert("worker".into(), serde_json::Value::String(agent.into()));
        *self.graph.node_weight_mut(idx).unwrap() = w;
        self.log_fih(
            "claim_intent",
            &serde_json::to_string(&(intent_id.to_string(), agent.to_string()))
                .unwrap_or_default(),
        );
        Ok(())
    }

    fn heartbeat(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let idx = self
            .resolve_intent_name(intent_id)
            .ok_or_else(|| BlackboardError::NotFound(format!("Intent: {intent_id}")))?;
        let w = self.graph.node_weight(idx).unwrap();
        match w.properties.get("worker").and_then(|v| v.as_str()) {
            Some(w) if w != agent => {
                return Err(BlackboardError::Conflict(format!("Claimed by {w}")));
            }
            None => return Err(BlackboardError::Forbidden("Not claimed".into())),
            _ => {}
        }
        Ok(())
    }

    fn release_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let idx = self
            .resolve_intent_name(intent_id)
            .ok_or_else(|| BlackboardError::NotFound(format!("Intent: {intent_id}")))?;
        let w = self.graph.node_weight(idx).unwrap();
        if let Some(worker_name) = w.properties.get("worker").and_then(|v| v.as_str())
            && worker_name == agent
        {
            let mut w = w.clone();
            w.properties.remove("worker");
            *self.graph.node_weight_mut(idx).unwrap() = w;
        }
        self.log_fih(
            "release_intent",
            &serde_json::to_string(&(intent_id.to_string(), agent.to_string()))
                .unwrap_or_default(),
        );
        Ok(())
    }

    fn conclude_intent(
        &mut self,
        intent_id: &str,
        result: &str,
    ) -> Result<(Fact, Vec<Intent>), BlackboardError> {
        let idx = self
            .resolve_intent_name(intent_id)
            .ok_or_else(|| BlackboardError::NotFound(format!("Intent: {intent_id}")))?;
        let intent = self.read_intent(idx).unwrap();

        let mut w = self.graph.node_weight(idx).unwrap().clone();
        w.properties.insert(
            "concluded_at".into(),
            serde_json::Value::String("now".into()),
        );
        w.properties.remove("worker");
        *self.graph.node_weight_mut(idx).unwrap() = w;

        let fact = Fact {
            id: FihHash(format!("fact_{}", intent.id.0)),
            origin: "Layer1".into(),
            content: result.into(),
            creator: intent.creator.clone(),
        };
        self.add_fact(&fact);
        self.log_fih(
            "conclude_intent",
            &serde_json::to_string(&(intent_id.to_string(), result.to_string()))
                .unwrap_or_default(),
        );

        let follow_ups = if !result.contains("done") {
            vec![Intent {
                id: FihHash(format!("intent_{}_next", intent.id.0)),
                from_facts: vec![fact.id.0.clone()],
                description: format!("Follow-up: {result}"),
                creator: intent.creator.clone(),
                worker: None,
                concluded_at: None,
            }]
        } else {
            Vec::new()
        };

        Ok((fact, follow_ups))
    }

    fn read_state(&self) -> BoardState {
        let facts: Vec<Fact> = self
            .graph
            .node_indices()
            .filter_map(|idx| self.read_fact(idx))
            .collect();
        let intents: Vec<Intent> = self
            .graph
            .node_indices()
            .filter_map(|idx| self.read_intent(idx))
            .collect();
        let hints: Vec<Hint> = self
            .graph
            .node_indices()
            .filter_map(|idx| {
                let w = self.graph.node_weight(idx)?;
                if w.label != "Hint" {
                    return None;
                }
                Some(Hint {
                    id: FihHash(w.name.clone()),
                    content: w.properties.get("content")?.as_str()?.into(),
                    creator: w.properties.get("creator")?.as_str()?.into(),
                })
            })
            .collect();

        BoardState {
            facts,
            intents,
            hints,
        }
    }
}

impl GraphAccess for GraphBlackboard {
    fn node_indices(&self) -> Vec<NodeIndex> {
        self.graph.node_indices().collect()
    }
    fn edge_indices(&self) -> Vec<EdgeIndex> {
        self.graph.edge_indices().collect()
    }
    fn node_weight(&self, idx: NodeIndex) -> Option<&NodeWeight> {
        self.graph.node_weight(idx)
    }
    fn edge_weight(&self, idx: EdgeIndex) -> Option<&EdgeWeight> {
        self.graph.edge_weight(idx)
    }
    fn edge_endpoints(&self, idx: EdgeIndex) -> Option<(NodeIndex, NodeIndex)> {
        self.graph.edge_endpoints(idx)
    }
    fn neighbors_undirected(&self, idx: NodeIndex) -> Vec<NodeIndex> {
        self.graph.neighbors_undirected(idx).collect()
    }
    fn edges_directed(&self, idx: NodeIndex, outgoing: bool) -> Vec<EdgeIndex> {
        let dir = if outgoing {
            petgraph::Direction::Outgoing
        } else {
            petgraph::Direction::Incoming
        };
        self.graph
            .edges_directed(idx, dir)
            .map(|e| e.id())
            .collect()
    }
    fn add_node(&mut self, weight: NodeWeight) -> NodeIndex {
        self.graph.add_node(weight)
    }
    fn add_edge(&mut self, from: NodeIndex, to: NodeIndex, weight: EdgeWeight) -> EdgeIndex {
        self.graph.add_edge(from, to, weight)
    }
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_submit_and_read_fact() {
        let mut bb = GraphBlackboard::new();
        let fact = Fact {
            id: FihHash("f001".into()),
            origin: "Layer1".into(),
            content: "Observation validated".into(),
            creator: "agent-a".into(),
        };
        let id = Blackboard::submit_fact(&mut bb, &fact);
        assert_eq!(id.0, "f001");
        let state = bb.read_state();
        assert_eq!(state.facts.len(), 1);
        assert_eq!(state.facts[0].content, "Observation validated");
    }

    #[test]
    fn test_intent_lifecycle() {
        let mut bb = GraphBlackboard::new();
        let fact = Fact {
            id: FihHash("f001".into()),
            origin: "L1".into(),
            content: "baseline".into(),
            creator: "a".into(),
        };
        bb.submit_fact(&fact);

        let intent = Intent {
            id: FihHash("i001".into()),
            from_facts: vec!["f001".into()],
            description: "test hypothesis".into(),
            creator: "agent-a".into(),
            worker: None,
            concluded_at: None,
        };
        bb.submit_intent(&intent).unwrap();
        assert!(bb.claim_intent("i001", "worker-1").is_ok());
        assert!(bb.heartbeat("i001", "worker-1").is_ok());
        assert!(bb.claim_intent("i001", "worker-2").is_err());

        let (new_fact, follow_ups) = bb.conclude_intent("i001", "hypothesis validated").unwrap();
        assert_eq!(new_fact.content, "hypothesis validated");
        assert_eq!(follow_ups.len(), 1);
    }

    #[test]
    fn test_submit_intent_without_facts_fails() {
        let mut bb = GraphBlackboard::new();
        let intent = Intent {
            id: FihHash("orphan".into()),
            from_facts: vec![],
            description: "no evidence".into(),
            creator: "a".into(),
            worker: None,
            concluded_at: None,
        };
        assert!(Blackboard::submit_intent(&mut bb, &intent).is_err());
    }

    #[test]
    fn test_read_state_empty() {
        let bb = GraphBlackboard::new();
        let state = bb.read_state();
        assert!(state.facts.is_empty());
        assert!(state.intents.is_empty());
        assert!(state.hints.is_empty());
    }
}
