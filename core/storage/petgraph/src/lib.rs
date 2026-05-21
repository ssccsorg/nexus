// nexus-storage-petgraph — Petgraph-backed FIH storage.

pub mod petgraph_storage;
pub mod graph_access;
pub mod weight;

pub use petgraph_storage::PetgraphStorage;
pub use graph_access::GraphAccess;
pub use weight::{EdgeWeight, NodeWeight, Record};
