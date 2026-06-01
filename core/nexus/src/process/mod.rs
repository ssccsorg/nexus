// nexus-process — OODA loop runtime + stigmergy logic.
//
// Data flows through layers: model → graph → process
//   model:   what data is (FIH types, detection traits)
//   graph:   how data is stored and queried (Cypher, petgraph)
//   process: how data drives action (scheduling, eviction, stigmergy)

pub mod scheduler;
pub mod eviction;
pub mod error;
pub mod tasks;

pub use scheduler::Scheduler;
pub use error::ProcessError;
