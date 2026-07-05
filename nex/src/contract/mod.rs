// ── Contract Layer: Governance gate + Evidence chain + Hint engine ─────
//
// Governance primitives for the FIH state space. These are assembled onto
// FihStorage as optional composition (see `FihStorage::with_governance`),
// following the same pattern as `core` storage traits.
//
// The three primitives:
//
//   GovernanceGate  — schema-based write admission (raw data → schemed-segment)
//   HintEngine      — constraint evaluation during Intent resolution
//   EvidenceChain   — append-only SHA-256 chain for tamper evidence
//
// wasm32-unknown-unknown: All types in this module compile under wasm
// because they use only std primitives + sha2 (pure Rust, no host fns).

pub mod evidence;
pub mod gate;
pub mod hint;
pub mod lifecycle;

pub use evidence::{EvidenceChain, EvidenceEntry};
pub use gate::{GovernanceBypassError, GovernanceGate};
pub use hint::{HintEngine, HintRule};
pub use lifecycle::{HealthStatus, NexConfig, NexInstanceInfo, NexLifecycle};
