// ── nex: process layer ─────────────────────────────────────────────────
//
// nex is the OODA loop process layer. It depends on nex-fih for the state
// model and its contracts, and provides the scheduler runtime that drives
// detection tasks over a Blackboard.
//
// It names states, not the layer that holds them: a storage implementation is a
// node on the network, reached over the protocol, and nothing here re-exports one.
//
// Platform adaptation is transparent via nex-fih's Cell2<T>:
//   Mutex<T> on native, RefCell<T> on wasm.
//   Same borrow()/borrow_mut() API regardless of platform.

pub mod error;
pub mod eviction;
pub mod scheduler;
pub mod tasks;

pub use error::ProcessError;
pub use scheduler::Scheduler;

/// Backward compatibility: old `nex::process::scheduler` and `nex::process::tasks` paths.
pub mod process {
    pub use crate::{scheduler, tasks};
}

// ── Backward-compatible module aliases ──────────────────────────────────
//
// `nex::io` re-exports nex-io so a consumer keeps one import root for the
// transport-independent IO seam.

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

// ── Top-level re-exports for the state model ───────────────────────────
//
// The model and its contracts, so a consumer of the process layer does not
// reach past it for `Fact`, `Intent`, or a governance type. Nothing that holds
// records is re-exported here.

pub use nex_fih::{
    Blackboard, BlackboardError, BoardState, Content, ContentJsonExt, ContradictionDetection,
    CoordId, DetectionCapable, DetectionOutput, EvidenceChain, EvidenceEntry, Fact, FihContract,
    FihHash, GapDetection, GovernanceGate, HealthStatus, Hint, HintEngine, HintRule, Intent,
    NexConfig, NexInstanceInfo, NexLifecycle, Query, RecordLoad, SemanticStore,
    StateChangeDetection, TaskStates,
};

// IO re-exports
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub use nex_io::FsIo;
pub use nex_io::{BlockingIo, FileIo, SyncFileIo, WriteOp};
