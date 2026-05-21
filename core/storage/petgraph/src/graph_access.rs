// nexus-graph — GraphAccess trait: petgraph query interface for Cypher executor.

use crate::weight::{EdgeWeight, NodeWeight};
use petgraph::graph::{EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;

/// Access interface for petgraph queries (Cypher executor).
///
/// All methods return owned values so the trait can be implemented for
/// types behind a Mutex (e.g. DefaultBlackboard).
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
