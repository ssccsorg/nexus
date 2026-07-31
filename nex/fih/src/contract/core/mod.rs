// ── Contract core primitives ───────────────────────────────────────────
//
// Implementation-level governance building blocks. These are pure
// primitives with no paradigm-specific coupling. Higher-level FIH
// contracts (contract/fih.rs) compose these for FIH use cases.
// Other nex-apps can compose them for their own paradigms.
//
// Note: NexLifecycle, NexConfig, NexInstanceInfo, and HealthStatus
// are defined in fih-model and re-exported from the crate root.

pub mod evidence;
pub mod gate;
pub mod hint;
pub(crate) mod util;

pub use evidence::{EvidenceChain, EvidenceEntry};
pub use gate::{GovernanceBypassError, GovernanceGate};
pub use hint::{HintEngine, HintRule};
