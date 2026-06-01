// nexus-graph — GraphRead / GraphWrite traits: petgraph query and mutation interfaces for Cypher executor.

use super::weight::{EdgeWeight, NodeWeight};
use petgraph::graph::{EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;
use std::sync::{RwLockReadGuard, RwLockWriteGuard};

/// Read-only access interface for petgraph queries (Cypher executor).
///
/// All methods return owned values so the trait can be implemented for
/// types behind a Mutex (e.g. DefaultBlackboard).
pub trait GraphRead {
    fn node_indices(&self) -> Vec<NodeIndex>;
    fn edge_indices(&self) -> Vec<EdgeIndex>;
    fn node_weight(&self, idx: NodeIndex) -> Option<NodeWeight>;
    fn edge_weight(&self, idx: EdgeIndex) -> Option<EdgeWeight>;
    fn edge_endpoints(&self, idx: EdgeIndex) -> Option<(NodeIndex, NodeIndex)>;
    fn neighbors_undirected(&self, idx: NodeIndex) -> Vec<NodeIndex>;
    fn edges_directed(&self, idx: NodeIndex, outgoing: bool) -> Vec<EdgeIndex>;
}

/// Mutable mutation interface for petgraph.
pub trait GraphWrite {
    fn add_node(&mut self, weight: NodeWeight) -> NodeIndex;
    fn add_edge(&mut self, from: NodeIndex, to: NodeIndex, weight: EdgeWeight) -> EdgeIndex;
}

impl GraphRead for petgraph::Graph<NodeWeight, EdgeWeight> {
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
}

impl<'a> GraphRead for RwLockReadGuard<'a, petgraph::Graph<NodeWeight, EdgeWeight>> {
    fn node_indices(&self) -> Vec<NodeIndex> {
        petgraph::Graph::node_indices(&**self).collect()
    }

    fn edge_indices(&self) -> Vec<EdgeIndex> {
        petgraph::Graph::edge_indices(&**self).collect()
    }

    fn node_weight(&self, idx: NodeIndex) -> Option<NodeWeight> {
        petgraph::Graph::node_weight(&**self, idx).cloned()
    }

    fn edge_weight(&self, idx: EdgeIndex) -> Option<EdgeWeight> {
        petgraph::Graph::edge_weight(&**self, idx).cloned()
    }

    fn edge_endpoints(&self, idx: EdgeIndex) -> Option<(NodeIndex, NodeIndex)> {
        petgraph::Graph::edge_endpoints(&**self, idx)
    }

    fn neighbors_undirected(&self, idx: NodeIndex) -> Vec<NodeIndex> {
        petgraph::Graph::neighbors_undirected(&**self, idx).collect()
    }

    fn edges_directed(&self, idx: NodeIndex, outgoing: bool) -> Vec<EdgeIndex> {
        let dir = if outgoing {
            petgraph::Direction::Outgoing
        } else {
            petgraph::Direction::Incoming
        };
        petgraph::Graph::edges_directed(&**self, idx, dir)
            .map(|e| e.id())
            .collect()
    }
}

impl<'a> GraphWrite for RwLockWriteGuard<'a, petgraph::Graph<NodeWeight, EdgeWeight>> {
    fn add_node(&mut self, weight: NodeWeight) -> NodeIndex {
        petgraph::Graph::add_node(&mut **self, weight)
    }

    fn add_edge(&mut self, from: NodeIndex, to: NodeIndex, weight: EdgeWeight) -> EdgeIndex {
        petgraph::Graph::add_edge(&mut **self, from, to, weight)
    }
}

impl GraphWrite for petgraph::Graph<NodeWeight, EdgeWeight> {
    fn add_node(&mut self, weight: NodeWeight) -> NodeIndex {
        self.add_node(weight)
    }

    fn add_edge(&mut self, from: NodeIndex, to: NodeIndex, weight: EdgeWeight) -> EdgeIndex {
        self.add_edge(from, to, weight)
    }
}
