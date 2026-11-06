#![no_std]
extern crate alloc;

// When the `std` feature is enabled, expose the standard library for the
// host-side pieces that legitimately need it (SystemTime, block_on paths).
#[cfg(feature = "std")]
extern crate std;

// ── nex-fih: the state model and its contracts ──────────────────────────
//
// The model this network speaks: Fact, Intent, Hint, identity, the read surface, and the
// governance primitives. It holds nothing.
//
//   core/       — the entity-store family and the ordered index
//   contract/   — GovernanceGate, EvidenceChain, HintEngine, FihContract, the read surface
//   helper/     — ContentJsonExt
//   io/         — IO re-exports from nex-io (compatibility shim)
//   detection   — DetectionCapable trait family
//   interner    — Deprecated string interner (moved from nexus-model)
//
// An implementation of the storage contract is a node, and it lives in storage/ref here
// or in the storage layer it belongs to.

// Re-export everything from fih-model (pure types + traits)
pub use fih_model::*;

// Remaining modules defined in this crate
pub mod contract;
pub mod core;
pub mod detection;
pub mod helper;
pub mod interner;
pub mod io;

// Re-exports of items still defined in nex-fih
pub use contract::core::{
    EvidenceChain, EvidenceEntry, GovernanceBypassError, GovernanceGate, HintEngine, HintRule,
};
pub use contract::fih::FihContract;
pub use contract::state::{FactById, FactFraming, FactWalk};
pub use core::index::{Cell2, OrderedIndex};
pub use core::{CoordEntityStore, EntityStore, MapEntityStore, MemoryEntityStore};
pub use detection::{
    ContradictionDetection, DetectionCapable, DetectionCheckpoint, DetectionOutput, GapDetection,
    StateChangeDetection, TaskStates,
};
pub use helper::ContentJsonExt;
