// ── fih-model: FIH primitive types and storage traits ──────────────────
//
// This crate defines the pure type layer and storage capability traits
// for the FIH (Fact-Intent-Hint) paradigm. It has zero knowledge of
// nex-specific IO, core storage engines, or any runtime implementation.
//
// Modules
//   fih/       — Fact, Intent, Hint, CoordId, FihHash, Content, BoardState
//   error/     — BlackboardError
//   clock/     — Now trait (platform-independent timestamp abstraction)
//   blackboard — Blackboard aggregate trait
//   storage/   — Storage capability traits: StorageRead, FactCapable, ...
//                ColdStorage, NullStorage, async counterparts
//   semantic/  — SemanticStore, RecordLoad, Query, FihRecordLoad
//   contract/  — NexLifecycle, NexConfig, NexInstanceInfo, HealthStatus

pub mod blackboard;
pub mod clock;
pub mod contract;
pub mod error;
pub mod fih;
pub mod semantic;
pub mod storage;

pub use blackboard::Blackboard;
pub use clock::Now;
pub use contract::core::{HealthStatus, NexConfig, NexInstanceInfo, NexLifecycle};
pub use error::BlackboardError;
pub use fih::{BoardState, Content, CoordId, Fact, FihHash, Hint, Intent};
pub use semantic::SemanticStore;
pub use semantic::fih::FihRecordLoad;
pub use semantic::record::{Query, RecordLoad};
pub use storage::*;
