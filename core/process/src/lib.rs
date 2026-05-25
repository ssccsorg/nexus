// nexus-process — OODA loop runtime + stigmergy logic.
//
// Data flows through layers: model → graph → process
//   model:   what data is (FIH types, detection traits)
//   graph:   how data is stored and queried (Cypher, petgraph)
//   process: how data drives action (scheduling, eviction, stigmergy)

pub mod error;
pub mod eviction;
pub mod scheduler;
pub mod tasks;

pub use error::ProcessError;
