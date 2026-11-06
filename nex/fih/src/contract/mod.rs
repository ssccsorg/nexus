// ── Contract module ────────────────────────────────────────────────────
//
// Contract layer: governance primitives and paradigm-specific compositions.
//
//   core/     ← primitives: GovernanceGate, HintEngine, EvidenceChain
//   fih.rs    ← FIH-specific defaults (schemas, constraint factories)
//   state.rs  ← the state network's read surface
//
// nex-apps import core primitives directly for custom contracts,
// or use fih.rs for ready-made FIH governance.

pub mod core;
pub mod fih;
pub mod state;

pub use core::*;
pub use state::{FactById, FactWalk};

// Backward-compatible re-exports from fih-model (these types are defined
// in fih-model and re-exported at the crate root of nex-fih, but also
// available here for legacy paths like `nex::contract::HealthStatus`).
pub use crate::{HealthStatus, NexConfig, NexInstanceInfo, NexLifecycle};
