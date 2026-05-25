// nexus-storage-petgraph — Petgraph-backed FIH storage.

pub mod graph_access;
pub mod petgraph_storage;
pub mod snapshot;
pub mod weight;

pub use graph_access::{GraphRead, GraphWrite};
pub use petgraph_storage::PetgraphStorage;
pub use snapshot::{Snapshottable, StorageSnapshot};
pub use weight::{EdgeWeight, NodeWeight, Record};
