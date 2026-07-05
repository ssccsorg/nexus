// ── Contract Layer: Governance primitives ──────────────────────────────
//
// Pure governance primitives for the FIH state space. These are
// implementation-level building blocks. The governance wrapper that
// assembles them onto FihStorage lives in storage/contract.rs, following
// the same pattern as storage/fih.rs (FihBlackboard).
//
// Primitives:
//   GovernanceGate  — schema-based write admission (raw data → schemed-segment)
//   HintEngine      — constraint evaluation during Intent resolution
//   EvidenceChain   — append-only SHA-256 chain for tamper evidence
//
// wasm32-unknown-unknown: All types compile under wasm (std + sha2).

pub mod evidence;
pub mod gate;
pub mod hint;
pub mod lifecycle;

pub use evidence::{EvidenceChain, EvidenceEntry};
pub use gate::{GovernanceBypassError, GovernanceGate};
pub use hint::{HintEngine, HintRule};
pub use lifecycle::{HealthStatus, NexConfig, NexInstanceInfo, NexLifecycle};
