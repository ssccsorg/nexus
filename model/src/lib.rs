// nexus-model — Blackboard trait, capability-based Storage traits, and FIH primitives.
//
// Pure interfaces, no storage backend. Both nexus-graph and nexus-storage-composite
// depend on this crate only.
//
// # Capability-based trait design
//
// Instead of a monolithic Storage trait, functionality is split into fine-grained
// capability traits. Each backend implements only what it provides.
//
//   StorageRead                  — core: project_id, read_state
//    ├── FactCapable             — submit_fact
//    ├── IntentCapable           — submit/claim/heartbeat/release/conclude
//    ├── HintCapable             — submit_hint
//    ├── FilterCapable           — read_state_filtered (partial reads)
//    ├── ScanCapable             — scan_partition (bulk reads)
//    ├── EvictCapable            — approximate_size + evict_before
//    ├── FlushCapable            — flush_since (incremental export)
//    └── TimeRangeCapable        — time_range (hot/cold routing)
//
//   Aggregate: FihPersistence = FactCapable + IntentCapable + HintCapable
//   Aggregate: HotStorage     = FihPersistence + EvictCapable
//   Aggregate: ColdStorage    = FihPersistence + FilterCapable
//
// # Detection capability traits (mirrors storage pattern)
//
//   DetectionCapable             — base: name + orient (all detectors)
//    ├── GapDetection            — orphan/cross-origin gap discovery
//    ├── ContradictionDetection  — conflicting claims on same topic
//    └── StateChangeDetection    — count-based change triggers (Cairn pattern)
//
//   Aggregate: FullDetection = GapDetection + ContradictionDetection + StateChangeDetection

pub mod blackboard;
pub mod clock;
pub mod detection;
pub mod error;
pub mod fih;
pub mod interner;
pub mod storage;

pub use blackboard::Blackboard;
pub use clock::{Now, SystemClock};
pub use detection::{
    ContradictionDetection, DetectionCapable, DetectionCheckpoint, DetectionOutput, FullDetection,
    GapDetection, StateChangeDetection, TaskStates,
};
pub use error::BlackboardError;
pub use fih::{BoardState, Content, Fact, FihHash, Hint, Intent};
#[allow(deprecated)]
pub use interner::Interner;
pub use storage::async_impl::{
    AsyncEvictCapable, AsyncFactCapable, AsyncFilterCapable, AsyncFlushCapable,
    AsyncGovernanceCapable, AsyncHintCapable, AsyncIntentCapable, AsyncScanCapable,
    AsyncStorageRead, AsyncTimeRangeCapable,
};
pub use storage::*;
