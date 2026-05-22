// nexus-process — OODA loop runtime + stigmergy logic.
//
// Data flows through layers: model → graph → process
//   model:   what data is (FIH types)
//   graph:   how data is stored and queried (Cypher, petgraph)
//   process: how data drives action (scheduling, eviction, stigmergy)
//
// Modules
// =======
//   scheduler/  ← OODA polling loop, heartbeat monitor, Intent dispatch
//   eviction/   ← flush + evict_before memory management
//   tasks/      ← stigmergy detectors (gap detection, knowledge promotion)

pub mod scheduler;
pub mod eviction;
