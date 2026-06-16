// nexus-petgraph — Petgraph-backed HotStorage
//
// petgraph-based HotStorage implementation for in-memory FIH state
// storage with Cypher-compatible graph traversal.
//
// Module structure:
//   storage.rs — PetgraphStorage: core implementation
//   weight.rs — EdgeWeight, NodeWeight: petgraph weight types
//   snapshot.rs — StorageSnapshot, Snapshottable: snapshot/restore
//   graph_access.rs — GraphRead, GraphWrite: re-exported from model

pub use nexus_model::storage::{GraphRead, GraphWrite};

pub mod graph_access;
mod snapshot;
mod storage;
mod weight;

pub use snapshot::{Snapshottable, StorageSnapshot};
pub use storage::{PetgraphStorage, SharedGraph, read_graph, write_graph};
pub use weight::{EdgeWeight, NodeWeight};
