use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLockReadGuard;
use std::sync::RwLockWriteGuard;

/// Weight type for petgraph nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeWeight {
    pub name: String,
    pub label: String,
    pub properties: HashMap<String, crate::Content>,
}

/// Weight type for petgraph edges.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeWeight {
    pub rel_type: String,
    pub properties: HashMap<String, crate::Content>,
}

/// Read-only access to a petgraph storage backend.
///
/// Access is gained through `graph()`, which returns a reference to the
/// underlying `petgraph::Graph`. Callers can then use petgraph's own
/// methods (`node_indices()`, `node_weight()`, etc.) on the returned
/// reference.
pub trait GraphRead {
    fn graph(&self) -> &petgraph::Graph<NodeWeight, EdgeWeight>;
}

/// Mutable access to a petgraph storage backend.
///
/// Access is gained through `graph_mut()`, which returns a mutable
/// reference to the underlying `petgraph::Graph`.
pub trait GraphWrite {
    fn graph_mut(&mut self) -> &mut petgraph::Graph<NodeWeight, EdgeWeight>;
}

// ── Implementations ───────────────────────────────────────────────────────

impl GraphRead for petgraph::Graph<NodeWeight, EdgeWeight> {
    fn graph(&self) -> &petgraph::Graph<NodeWeight, EdgeWeight> {
        self
    }
}

impl GraphWrite for petgraph::Graph<NodeWeight, EdgeWeight> {
    fn graph_mut(&mut self) -> &mut petgraph::Graph<NodeWeight, EdgeWeight> {
        self
    }
}

impl<'a> GraphRead for RwLockReadGuard<'a, petgraph::Graph<NodeWeight, EdgeWeight>> {
    fn graph(&self) -> &petgraph::Graph<NodeWeight, EdgeWeight> {
        self
    }
}

impl<'a> GraphWrite for RwLockWriteGuard<'a, petgraph::Graph<NodeWeight, EdgeWeight>> {
    fn graph_mut(&mut self) -> &mut petgraph::Graph<NodeWeight, EdgeWeight> {
        self
    }
}

/// Blanket impl: any reference to a GraphRead is also GraphRead.
impl<T: GraphRead + ?Sized> GraphRead for &T {
    fn graph(&self) -> &petgraph::Graph<NodeWeight, EdgeWeight> {
        (**self).graph()
    }
}

/// Blanket impl: Arc<T> where T: GraphRead is also GraphRead.
impl<T: GraphRead + ?Sized> GraphRead for std::sync::Arc<T> {
    fn graph(&self) -> &petgraph::Graph<NodeWeight, EdgeWeight> {
        (**self).graph()
    }
}
