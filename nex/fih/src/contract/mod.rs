// ── Contract module ────────────────────────────────────────────────────
//
// Contract layer: governance primitives and paradigm-specific compositions.
//
//   core/     ← primitives: GovernanceGate, HintEngine, EvidenceChain
//   fih.rs    ← FIH-specific defaults (schemas, constraint factories)
//
// nex-apps import core primitives directly for custom contracts,
// or use fih.rs for ready-made FIH governance.

pub mod core;
pub mod fih;

pub use core::*;

// Backward-compatible re-exports from fih-model (these types are defined
// in fih-model and re-exported at the crate root of nex-fih, but also
// available here for legacy paths like `nex::contract::HealthStatus`).
pub use crate::{HealthStatus, NexConfig, NexInstanceInfo, NexLifecycle};
