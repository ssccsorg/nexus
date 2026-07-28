// ── nex-fih: FIH primitives ─────────────────────────────────────────────
//
// FIH (Fact-Intent-Hint) primitive types, storage traits, and
// implementation layer. This crate defines everything FIH-related:
//
//   storage/    — FIH storage traits (StorageRead, FactCapable, etc.)
//   core/       — FihStorage, EntityStore, FihSession, records
//   semantic/   — SemanticStore, RecordLoad, Query
//   contract/   — GovernanceGate, EvidenceChain, HintEngine, FihContract
//   helper/     — ContentJsonExt
//   io/         — IO re-exports from nex-io (compatibility shim)
//   blackboard  — Blackboard aggregate trait
//   detection   — DetectionCapable trait family
//   error       — BlackboardError
//   fih         — Fact, Intent, Hint, Content, FihHash, BoardState
//   interner    — Deprecated string interner (moved from nexus-model)

pub mod blackboard;
pub mod contract;
pub mod core;
pub mod detection;
pub mod error;
pub mod fih;
pub mod helper;
pub mod interner;
pub mod io;
pub mod semantic;
pub mod storage;

pub use blackboard::Blackboard;
pub use contract::core::{
    EvidenceChain, EvidenceEntry, GovernanceBypassError, GovernanceGate, HealthStatus, HintEngine,
    HintRule, NexConfig, NexInstanceInfo, NexLifecycle,
};
pub use contract::fih::FihContract;
pub use core::entity_store::{CoordEntityStore, EntityStore, MemoryEntityStore};
pub use core::export::{export_from_io, import_into_io};
pub use core::fih_blackboard::FihBlackboard;
pub use core::index::{Cell2, OrderedIndex};
pub use core::intent_status::IntentStatus;
pub use core::record::{ContentMeta, FactRecord, HintRecord, IntentRecord};
pub use core::session::FihSession;
pub use core::store::{ChainEntry, FihStorage};
pub use detection::{
    ContradictionDetection, DetectionCapable, DetectionCheckpoint, DetectionOutput, GapDetection,
    StateChangeDetection, TaskStates,
};
pub use error::BlackboardError;
pub use fih::{BoardState, Content, CoordId, Fact, FihHash, Hint, Intent};
pub use helper::ContentJsonExt;
pub use semantic::SemanticStore;
pub use semantic::record::{Query, RecordLoad};
pub use storage::async_impl::{
    AsyncEvictCapable, AsyncFactCapable, AsyncFilterCapable, AsyncFlushCapable, AsyncHintCapable,
    AsyncIntentCapable, AsyncScanCapable, AsyncStorageRead, AsyncTimeRangeCapable,
};
pub use storage::*;
