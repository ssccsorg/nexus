// nexus-graph — canonical graph store types and the Blackboard.
//
// The Blackboard is a collaboration space, not a passive data container.
// Agents interact exclusively through Fact, Intent, and Hint primitives.
// Direct graph manipulation is forbidden by the type system.

use petgraph::graph::{EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(test)]
fn now_iso() -> String {
    "0".into()
}
use std::time::{SystemTime, UNIX_EPOCH};

// ── Timestamp helper ──────────────────────────────────────────────────────


// ── Primitives: Fact / Intent / Hint ─────────────────────────────────────

/// A verified statement: experiment result, discovered entity, validated relation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    /// Content-addressable ID: H(origin + content + timestamp).
    pub id: String,
    /// The Blackboard or context that produced this Fact.
    /// Layer 0 = raw petgraph, Layer 1 = domain Blackboard, Layer N = meta.
    pub origin: String,
    pub content: String,
    pub creator: String,
    pub created_at: String,
}

/// An exploration direction: gap, hypothesis, proposed action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub id: String,
    /// Grounded in these Fact IDs. Cannot exist without evidential basis.
    pub from_facts: Vec<String>,
    pub description: String,
    pub creator: String,
    pub worker: Option<String>,
    pub created_at: String,
    pub concluded_at: Option<String>,
}

/// A governance rule or human guidance. contract.nex rules, user feedback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hint {
    pub id: String,
    pub content: String,
    pub creator: String,
    pub created_at: String,
}

// ── Signal (Stigmergy trace) ─────────────────────────────────────────────

/// A Stigmergy signal deposited on the Blackboard.
/// Agents perceive signals left by other agents; no direct communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signal {
    /// The ID of the agent that deposited this signal.
    pub agent_id: String,
    /// Spatial or logical zone (e.g., "concept:Observation", "gap:orphan").
    pub zone: String,
    /// Arbitrary payload. Interpreted by perceiving agents.
    pub payload: String,
    /// Seconds since epoch when deposited.
    pub timestamp: u64,
    /// Multiplier applied per decay cycle. 1.0 = permanent, 0.5 = halves each cycle.
    pub decay_rate: f64,
}

// ── Node weights ──────────────────────────────────────────────────────────

/// Every node in the Blackboard carries a type tag and typed payload.
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

// ── Blackboard trait: the single interface ────────────────────────────────

pub trait Blackboard: Default {
    fn node_indices(&self) -> Vec<NodeIndex>;
    fn edge_indices(&self) -> Vec<EdgeIndex>;
    fn node_weight(&self, idx: NodeIndex) -> Option<&NodeWeight>;
    fn edge_weight(&self, idx: EdgeIndex) -> Option<&EdgeWeight>;
    fn edge_endpoints(&self, idx: EdgeIndex) -> Option<(NodeIndex, NodeIndex)>;
    fn neighbors_undirected(&self, idx: NodeIndex) -> Vec<NodeIndex>;
    fn edges_directed(&self, idx: NodeIndex, outgoing: bool) -> Vec<EdgeIndex>;
    fn add_node(&mut self, weight: NodeWeight) -> NodeIndex;
    fn add_edge(&mut self, from: NodeIndex, to: NodeIndex, weight: EdgeWeight) -> EdgeIndex;

    // ── Stigmergy ─────────────────────────────────────────────────────────
    fn deposit_signal(&mut self, signal: Signal);
    fn read_signals(&self, zone: &str) -> Vec<&Signal>;
    fn decay_signals(&mut self, multiplier: f64) -> usize;
    fn signal_count(&self) -> usize;
}

// ── Blackboard struct: wraps petgraph, enforces primitives ────────────────

pub struct BlackboardStore {
    graph: petgraph::Graph<NodeWeight, EdgeWeight>,
    signals: Vec<Signal>,
}

impl BlackboardStore {
    pub fn new() -> Self {
        Self {
            graph: petgraph::Graph::new(),
            signals: Vec::new(),
        }
    }

    // ── Fact operations ───────────────────────────────────────────────────

    pub fn add_fact(&mut self, fact: &Fact) -> NodeIndex {
        let mut props = HashMap::new();
        props.insert("origin".into(), serde_json::Value::String(fact.origin.clone()));
        props.insert("content".into(), serde_json::Value::String(fact.content.clone()));
        props.insert("creator".into(), serde_json::Value::String(fact.creator.clone()));
        let weight = NodeWeight {
            name: fact.id.clone(),
            label: "Fact".into(),
            properties: props,
        };
        self.graph.add_node(weight)
    }

    pub fn get_fact(&self, idx: NodeIndex) -> Option<Fact> {
        let w = self.graph.node_weight(idx)?;
        if w.label != "Fact" {
            return None;
        }
        Some(Fact {
            id: w.name.clone(),
            origin: w.properties.get("origin")?.as_str()?.into(),
            content: w.properties.get("content")?.as_str()?.into(),
            creator: w.properties.get("creator")?.as_str()?.into(),
            created_at: String::new(),
        })
    }

    pub fn find_facts_by_label(&self, label: &str) -> Vec<(NodeIndex, Fact)> {
        self.graph
            .node_indices()
            .filter_map(|idx| {
                let w = self.graph.node_weight(idx)?;
                if w.label != "Fact" {
                    return None;
                }
                let fact = Fact {
                    id: w.name.clone(),
                    origin: w.properties.get("origin")?.as_str()?.into(),
                    content: w.properties.get("content")?.as_str()?.into(),
                    creator: w.properties.get("creator")?.as_str()?.into(),
                    created_at: String::new(),
                };
                Some((idx, fact))
            })
            .filter(|(_, f)| f.origin == label || f.content.contains(label))
            .collect()
    }

    // ── Intent operations ─────────────────────────────────────────────────

    pub fn add_intent(&mut self, intent: &Intent) -> NodeIndex {
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
        let weight = NodeWeight {
            name: intent.id.clone(),
            label: "Intent".into(),
            properties: props,
        };
        let ni = self.graph.add_node(weight);

        // Connect to source Facts
        for fact_id in &intent.from_facts {
            if let Some(fact_idx) = self.find_node_by_name(fact_id) {
                let ew = EdgeWeight {
                    rel_type: "GROUNDED_IN".into(),
                    properties: HashMap::new(),
                };
                self.graph.add_edge(ni, fact_idx, ew);
            }
        }
        ni
    }

    pub fn get_intent(&self, idx: NodeIndex) -> Option<Intent> {
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
            created_at: String::new(),
            concluded_at: None,
        })
    }

    // ── Hint operations ───────────────────────────────────────────────────

    pub fn add_hint(&mut self, hint: &Hint) -> NodeIndex {
        let mut props = HashMap::new();
        props.insert("content".into(), serde_json::Value::String(hint.content.clone()));
        props.insert("creator".into(), serde_json::Value::String(hint.creator.clone()));
        let weight = NodeWeight {
            name: hint.id.clone(),
            label: "Hint".into(),
            properties: props,
        };
        self.graph.add_node(weight)
    }

    // ── Stigmergy operations ──────────────────────────────────────────────

    pub fn deposit_signal_store(&mut self, signal: Signal) {
        self.signals.push(signal);
    }

    pub fn read_signals_in_zone(&self, zone: &str) -> Vec<&Signal> {
        self.signals
            .iter()
            .filter(|s| s.zone == zone)
            .collect()
    }

    pub fn decay_signals_store(&mut self, _multiplier: f64) -> usize {
        let before = self.signals.len();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.signals.retain(|s| {
            let age = (now.saturating_sub(s.timestamp)) as f64;
            let strength = s.decay_rate.powf(age / 60.0);
            strength > 0.01
        });
        before - self.signals.len()
    }

    pub fn signal_count_store(&self) -> usize {
        self.signals.len()
    }

    // ── Utility ───────────────────────────────────────────────────────────

    fn find_node_by_name(&self, name: &str) -> Option<NodeIndex> {
        self.graph
            .node_indices()
            .find(|&idx| {
                self.graph
                    .node_weight(idx)
                    .is_some_and(|w| w.name == name)
            })
    }

    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }
}

impl Default for BlackboardStore {
    fn default() -> Self {
        Self::new()
    }
}

// ── Blackboard trait impl for BlackboardStore ─────────────────────────────

impl Blackboard for BlackboardStore {
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

    fn deposit_signal(&mut self, signal: Signal) {
        self.deposit_signal_store(signal);
    }
    fn read_signals(&self, zone: &str) -> Vec<&Signal> {
        self.read_signals_in_zone(zone)
    }
    fn decay_signals(&mut self, multiplier: f64) -> usize {
        self.decay_signals_store(multiplier)
    }
    fn signal_count(&self) -> usize {
        self.signal_count_store()
    }
}

// ── petgraph::Graph impl (bare graph, direct access for Cypher executor) ──

impl Blackboard for petgraph::Graph<NodeWeight, EdgeWeight> {
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
    fn deposit_signal(&mut self, _signal: Signal) {}
    fn read_signals(&self, _zone: &str) -> Vec<&Signal> {
        Vec::new()
    }
    fn decay_signals(&mut self, _multiplier: f64) -> usize {
        0
    }
    fn signal_count(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_and_get_fact() {
        let mut bb = BlackboardStore::new();
        let fact = Fact {
            id: "fact-001".into(),
            origin: "Layer1".into(),
            content: "Observation primitive validated on M1".into(),
            creator: "agent-gap".into(),
            created_at: now_iso(),
        };
        let idx = bb.add_fact(&fact);
        let retrieved = bb.get_fact(idx).unwrap();
        assert_eq!(retrieved.content, fact.content);
        assert_eq!(retrieved.origin, "Layer1");
    }

    #[test]
    fn test_add_intent_grounded_in_fact() {
        let mut bb = BlackboardStore::new();
        let fact = Fact {
            id: "fact-001".into(),
            origin: "Layer1".into(),
            content: "Normalized weight verified".into(),
            creator: "agent-test".into(),
            created_at: now_iso(),
        };
        bb.add_fact(&fact);

        let intent = Intent {
            id: "intent-001".into(),
            from_facts: vec!["fact-001".into()],
            description: "Test normalized weight under Field perturbation".into(),
            creator: "agent-gap".into(),
            worker: None,
            created_at: now_iso(),
            concluded_at: None,
        };
        let idx = bb.add_intent(&intent);
        let retrieved = bb.get_intent(idx).unwrap();
        assert_eq!(retrieved.from_facts, vec!["fact-001"]);
        assert_eq!(retrieved.description, intent.description);
    }

    #[test]
    fn test_signal_deposit_and_decay() {
        let mut bb = BlackboardStore::new();
        bb.deposit_signal(Signal {
            agent_id: "agent-a".into(),
            zone: "concept:Observation".into(),
            payload: "validated".into(),
            timestamp: 0,
            decay_rate: 0.5,
        });
        assert_eq!(bb.signal_count(), 1);
        let decayed = bb.decay_signals(0.5);
        assert!(decayed >= 0);
    }
}
