// ── nex: process layer ─────────────────────────────────────────────────
//
// nex is the OODA loop process layer. It depends on nex-fih for FIH
// primitives, storage, contracts, and IO, and provides the scheduler
// runtime that drives detection tasks over a Blackboard.
//
// All FIH types (FihStorage, FihBlackboard, Fact, Intent, etc.) are
// re-exported from nex-fih for consumer convenience.
//
// Platform adaptation is transparent via nex-fih's Cell2<T>:
//   Mutex<T> on native, RefCell<T> on wasm.
//   Same borrow()/borrow_mut() API regardless of platform.

pub mod process;

// ── Backward-compatible module aliases ──────────────────────────────────
//
// These modules re-export from nex-fih and nex-io so that existing
// consumers using paths like `nex::storage::core::FihStorage` or
// `nex::io::FileIo` continue to compile without changes.

/// Backward-compatible alias: shadow module preserving deep paths.
/// Consumers can use `nex::storage::core::FihStorage`, etc.
pub mod storage {
    /// Nested module path: nex::storage::core
    pub mod core {
        pub use nex_fih::core::*;
    }
    /// Nested module path: nex::storage::semantic
    pub mod semantic {
        pub use nex_fih::semantic::*;
    }
}

/// Backward-compatible alias: re-exports nex-io.
pub mod io {
    pub use nex_io::*;
    /// Nested module path: nex::io::file_io
    pub mod file_io {
        pub use nex_io::file_io::*;
    }
    /// Nested module path: nex::io::fs_io
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    pub mod fs_io {
        pub use nex_io::fs_io::*;
    }
}

/// Backward-compatible alias: re-exports nex-fih contract module.
pub mod contract {
    pub use nex_fih::contract::*;
}

/// Backward-compatible alias: re-exports nex-fih helper module.
pub mod helper {
    pub use nex_fih::helper::*;
}

// ── Top-level re-exports for common types ──────────────────────────────
//
// These allow `use nex::FihStorage` etc. without nesting.

pub use nex_fih::{
    Blackboard, BlackboardError, BoardState, Cell2, Content, ContentJsonExt,
    ContradictionDetection, CoordRef, DetectionCapable, DetectionOutput, EntityStore, EvictCapable,
    EvidenceChain, EvidenceEntry, Fact, FactCapable, FactRecord, FihBlackboard, FihContract,
    FihExport, FihHash, FihImport, FihSession, FihStorage, GapDetection, GovernanceGate,
    HealthStatus, Hint, HintCapable, HintEngine, HintRecord, HintRule, Intent, IntentCapable,
    IntentRecord, IntentStatus, MemoryEntityStore, NexConfig, NexInstanceInfo, NexLifecycle,
    OrderedIndex, Query, RecordLoad, SemanticStore, StateChangeDetection, StorageRead, TaskStates,
    export_from_io, import_into_io,
};

// IO re-exports
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub use nex_io::FsIo;
pub use nex_io::{FileIo, SyncFileIo, WriteOp};
