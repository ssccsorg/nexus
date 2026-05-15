// nexus-graph — canonical graph store types and traits.
//
// Defines the core data model (NodeWeight, EdgeWeight, Record) and the
// Blackboard trait that every graph backend must implement. The cypher
// executor depends on these types, not the other way around.

use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Canonical node weight ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeWeight {
    pub name: String,
    pub label: String,
    pub properties: HashMap<String, serde_json::Value>,
}

// ── Canonical edge weight ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeWeight {
    pub rel_type: String,
    pub properties: HashMap<String, serde_json::Value>,
}

// ── Record (query result row) ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub fields: HashMap<String, serde_json::Value>,
}

// ── Blackboard trait ───────────────────────────────────────────────────────

/// Trait for graphs that support node/edge iteration with petgraph NodeIndex.
///
/// Every graph backend (petgraph::Graph, Memgraph proxy, WASM stub) must
/// implement this trait. The cypher executor is generic over `G: Blackboard`.
pub trait Blackboard: Default {
    fn node_indices(&self) -> Vec<NodeIndex>;
    fn edge_indices(&self) -> Vec<petgraph::graph::EdgeIndex>;
    fn node_weight(&self, idx: NodeIndex) -> Option<&NodeWeight>;
    fn edge_weight(&self, idx: petgraph::graph::EdgeIndex) -> Option<&EdgeWeight>;
    fn edge_endpoints(&self, idx: petgraph::graph::EdgeIndex) -> Option<(NodeIndex, NodeIndex)>;
    fn neighbors_undirected(&self, idx: NodeIndex) -> Vec<NodeIndex>;
    fn edges_directed(&self, idx: NodeIndex, outgoing: bool) -> Vec<petgraph::graph::EdgeIndex>;
    fn add_node(&mut self, weight: NodeWeight) -> NodeIndex;
    fn add_edge(
        &mut self,
        from: NodeIndex,
        to: NodeIndex,
        weight: EdgeWeight,
    ) -> petgraph::graph::EdgeIndex;
}

// ── petgraph::Graph impl ──────────────────────────────────────────────────

impl Blackboard for petgraph::Graph<NodeWeight, EdgeWeight> {
    fn node_indices(&self) -> Vec<NodeIndex> {
        self.node_indices().collect()
    }

    fn edge_indices(&self) -> Vec<petgraph::graph::EdgeIndex> {
        self.edge_indices().collect()
    }

    fn node_weight(&self, idx: NodeIndex) -> Option<&NodeWeight> {
        petgraph::Graph::node_weight(self, idx)
    }

    fn edge_weight(&self, idx: petgraph::graph::EdgeIndex) -> Option<&EdgeWeight> {
        petgraph::Graph::edge_weight(self, idx)
    }

    fn edge_endpoints(&self, idx: petgraph::graph::EdgeIndex) -> Option<(NodeIndex, NodeIndex)> {
        self.edge_endpoints(idx)
    }

    fn neighbors_undirected(&self, idx: NodeIndex) -> Vec<NodeIndex> {
        self.neighbors_undirected(idx).collect()
    }

    fn edges_directed(&self, idx: NodeIndex, outgoing: bool) -> Vec<petgraph::graph::EdgeIndex> {
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

    fn add_edge(
        &mut self,
        from: NodeIndex,
        to: NodeIndex,
        weight: EdgeWeight,
    ) -> petgraph::graph::EdgeIndex {
        self.add_edge(from, to, weight)
    }
}
