// ── nex-fih: FIH primitives ─────────────────────────────────────────────
//
// FIH (Fact-Intent-Hint) primitive types, storage traits, and
// implementation layer. This crate extends fih-model with
// nex-specific implementations.
//
//   core/       — FihStorage, EntityStore, FihSession, records
//   contract/   — GovernanceGate, EvidenceChain, HintEngine, FihContract
//   helper/     — ContentJsonExt
//   io/         — IO re-exports from nex-io (compatibility shim)
//   detection   — DetectionCapable trait family
//   interner    — Deprecated string interner (moved from nexus-model)

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
pub use core::export::{export_from_io, import_into_io};
pub use core::fih_blackboard::FihBlackboard;
pub use core::index::{Cell2, OrderedIndex};
pub use core::intent_status::IntentStatus;
pub use core::record::{ContentMeta, FactRecord, HintRecord, IntentRecord};
pub use core::session::FihSession;
pub use core::store::FihStorage;
pub use core::{CoordEntityStore, EntityStore, MapEntityStore, MemoryEntityStore};
pub use detection::{
    ContradictionDetection, DetectionCapable, DetectionCheckpoint, DetectionOutput, GapDetection,
    StateChangeDetection, TaskStates,
};
pub use helper::ContentJsonExt;
