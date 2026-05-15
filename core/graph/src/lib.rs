#![allow(dead_code)]
// nexus-graph — canonical graph store types and the FIH Blackboard.
//
// FIH (Fact / Intent / Hint) is the universal IO interface.
// Every module reads and writes only these three types.
// The storage layer beneath is each backend's own concern.
//
// Layer structure:
//   Blackboard trait     — FIH lifecycle (public, stable)
//   GraphAccess trait    — raw graph operations (internal, cypher executor)
//   BlackboardStore      — petgraph impl of both
//   petgraph::Graph      — bare storage (implements GraphAccess only)

use petgraph::graph::{EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Hashing ──────────────────────────────────────────────────────────────

use std::hash::{DefaultHasher, Hash, Hasher};

/// Default content-addressable hashing for FIH primitives.
/// Backends can override by replacing `id` before submission.
pub fn fih_hash(fields: &[&str]) -> String {
    let mut s = DefaultHasher::new();
    for f in fields {
        f.hash(&mut s);
    }
    format!("{:x}", s.finish())
}

/// Compute content-addressable ID from all fields of a Fact.
pub fn fact_id(origin: &str, content: &str, creator: &str) -> String {
    fih_hash(&[origin, content, creator, "Fact"])
}

/// Compute content-addressable ID from all fields of an Intent.
pub fn intent_id(from_facts: &[String], description: &str, creator: &str) -> String {
    let mut all: Vec<String> = from_facts.to_vec();
    all.push(description.to_string());
    all.push(creator.to_string());
    all.push("Intent".into());
    fih_hash(&all.iter().map(|s| s.as_str()).collect::<Vec<_>>())
}

/// Compute content-addressable ID from all fields of a Hint.
pub fn hint_id(content: &str, creator: &str) -> String {
    fih_hash(&[content, creator, "Hint"])
}

// ── FIH Primitives ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    /// Content-addressable ID. Default: `fact_id(origin, content, creator)`.
    pub id: String,
    /// Origin Blackboard or context. Enables recursive multi-dimension linking.
    pub origin: String,
    pub content: String,
    pub creator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    /// Content-addressable ID. Default: `intent_id(from_facts, description, creator)`.
    pub id: String,
    /// Grounded in Fact IDs. Intents without evidence cannot exist.
    pub from_facts: Vec<String>,
    pub description: String,
    pub creator: String,
    pub worker: Option<String>,
    pub concluded_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hint {
    /// Content-addressable ID. Default: `hint_id(content, creator)`.
    pub id: String,
    pub content: String,
    pub creator: String,
}

/// Snapshot of the Blackboard at a point in time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardState {
    pub facts: Vec<Fact>,
    pub intents: Vec<Intent>,
    pub hints: Vec<Hint>,
}

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

// ── FIH Blackboard trait (public, stable interface) ──────────────────────

/// Every module reads and writes through this interface.
/// The Cairn-verified lifecycle: create → claim → heartbeat → conclude.
///
/// FIH is intentionally minimal: every field must be presentable by any backend.
/// Hashing strategy, timestamps, and storage details are backend concerns.
pub trait Blackboard: Default {
    /// Submit a Fact. Returns the assigned ID (backend-dependent hashing).
    fn submit_fact(&mut self, fact: &Fact) -> String;
    fn submit_hint(&mut self, hint: &Hint);

    fn submit_intent(&mut self, intent: &Intent) -> Result<String, String>;
    fn claim_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), String>;
    fn heartbeat(&mut self, intent_id: &str, agent: &str) -> Result<(), String>;
    fn release_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), String>;
    /// Resolve an Intent: produce new Facts and optionally spawn new Intents.
    fn conclude_intent(
        &mut self,
        intent_id: &str,
        result: &str,
    ) -> Result<(Fact, Vec<Intent>), String>;

    fn read_state(&self) -> BoardState;
}

// ── GraphAccess trait (internal — cypher executor) ───────────────────────

pub trait GraphAccess: Default {
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

// ── BlackboardStore (petgraph-backed, implements both traits) ────────────

pub struct BlackboardStore {
    graph: petgraph::Graph<NodeWeight, EdgeWeight>,
    signals: Vec<Signal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signal {
    pub agent_id: String,
    pub zone: String,
    pub payload: String,
    pub timestamp: u64,
    pub decay_rate: f64,
}

impl Default for BlackboardStore {
    fn default() -> Self {
        Self {
            graph: petgraph::Graph::new(),
            signals: Vec::new(),
        }
    }
}

impl BlackboardStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn find_node_by_name(&self, name: &str) -> Option<NodeIndex> {
        self.graph.node_indices().find(|&idx| {
            self.graph
                .node_weight(idx)
                .is_some_and(|w| w.name == name)
        })
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
            name: fact.id.clone(),
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
            name: intent.id.clone(),
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
            id: w.name.clone(),
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
            id: w.name.clone(),
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

impl Blackboard for BlackboardStore {
    fn submit_fact(&mut self, fact: &Fact) -> String {
        self.add_fact(fact);
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
            name: hint.id.clone(),
            label: "Hint".into(),
            properties: props,
        });
    }

    fn submit_intent(&mut self, intent: &Intent) -> Result<String, String> {
        if intent.from_facts.is_empty() {
            return Err("Intent must be grounded in at least one Fact".into());
        }
        self.add_intent(intent);
        Ok(intent.id.clone())
    }

    fn claim_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), String> {
        let idx = self
            .resolve_intent_name(intent_id)
            .ok_or_else(|| format!("Intent not found: {intent_id}"))?;
        let w = self.graph.node_weight(idx).unwrap();
        if let Some(existing) = w.properties.get("worker").and_then(|v| v.as_str()) {
            if existing != agent {
                return Err(format!("Intent claimed by {existing}"));
            }
        }
        let mut w = w.clone();
        w.properties
            .insert("worker".into(), serde_json::Value::String(agent.into()));
        *self.graph.node_weight_mut(idx).unwrap() = w;
        Ok(())
    }

    fn heartbeat(&mut self, intent_id: &str, agent: &str) -> Result<(), String> {
        let idx = self
            .resolve_intent_name(intent_id)
            .ok_or_else(|| format!("Intent not found: {intent_id}"))?;
        let w = self.graph.node_weight(idx).unwrap();
        match w.properties.get("worker").and_then(|v| v.as_str()) {
            Some(w) if w != agent => return Err(format!("Intent claimed by {w}")),
            None => return Err("Intent not claimed".into()),
            _ => {}
        }
        Ok(())
    }

    fn release_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), String> {
        let idx = self
            .resolve_intent_name(intent_id)
            .ok_or_else(|| format!("Intent not found: {intent_id}"))?;
        let w = self.graph.node_weight(idx).unwrap();
        if let Some(worker_name) = w.properties.get("worker").and_then(|v| v.as_str()) {
            if worker_name == agent {
                let mut w = w.clone();
                w.properties.remove("worker");
                *self.graph.node_weight_mut(idx).unwrap() = w;
            }
        }
        Ok(())
    }

    fn conclude_intent(
        &mut self,
        intent_id: &str,
        result: &str,
    ) -> Result<(Fact, Vec<Intent>), String> {
        let idx = self
            .resolve_intent_name(intent_id)
            .ok_or_else(|| format!("Intent not found: {intent_id}"))?;
        let intent = self.read_intent(idx).unwrap();

        let mut w = self.graph.node_weight(idx).unwrap().clone();
        w.properties.insert(
            "concluded_at".into(),
            serde_json::Value::String("now".into()),
        );
        *self.graph.node_weight_mut(idx).unwrap() = w;

        let fact = Fact {
            id: format!("fact_{}", intent.id),
            origin: "Layer1".into(),
            content: result.into(),
            creator: intent.creator.clone(),
        };
        self.add_fact(&fact);

        let follow_ups = if !result.contains("done") {
            vec![Intent {
                id: format!("intent_{}_next", intent.id),
                from_facts: vec![fact.id.clone()],
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
                    id: w.name.clone(),
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

impl GraphAccess for BlackboardStore {
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
        self.graph.edges_directed(idx, dir).map(|e| e.id()).collect()
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
        let mut bb = BlackboardStore::new();
        let fact = Fact {
            id: "f001".into(),
            origin: "Layer1".into(),
            content: "Observation validated".into(),
            creator: "agent-a".into(),
        };
        let id = Blackboard::submit_fact(&mut bb, &fact);
        assert_eq!(id, "f001");
        let state = bb.read_state();
        assert_eq!(state.facts.len(), 1);
        assert_eq!(state.facts[0].content, "Observation validated");
    }

    #[test]
    fn test_intent_lifecycle() {
        let mut bb = BlackboardStore::new();
        let fact = Fact {
            id: "f001".into(),
            origin: "L1".into(),
            content: "baseline".into(),
            creator: "a".into(),
        };
        bb.submit_fact(&fact);

        let intent = Intent {
            id: "i001".into(),
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
        let mut bb = BlackboardStore::new();
        let intent = Intent {
            id: "orphan".into(),
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
        let bb = BlackboardStore::new();
        let state = bb.read_state();
        assert!(state.facts.is_empty());
        assert!(state.intents.is_empty());
        assert!(state.hints.is_empty());
    }
}
