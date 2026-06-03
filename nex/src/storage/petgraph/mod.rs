pub mod graph_access;
pub mod petgraph_storage;
pub mod snapshot;
pub mod weight;

pub use nexus_model::storage::{GraphRead, GraphWrite};
pub use petgraph_storage::{PetgraphStorage, SharedGraph, read_graph, write_graph};
pub use snapshot::{Snapshottable, StorageSnapshot};
pub use weight::{EdgeWeight, NodeWeight};
