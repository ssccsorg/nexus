// ── nex: FIH Blackboard Storage Engine ───────────────────────────────
//
// nex is the core storage engine for the nexus FIH system.
// Each FihStorage instance owns its in-memory state and I/O channel.
//
// On native (macOS, Linux, WASIX): interior mutability uses Mutex.
// FihStorage<AsyncFileIo> is Send + Sync — safe to share across threads
// via Arc (e.g., axum server state).
//
// On wasm32-unknown-unknown: interior mutability uses RefCell.
// FihStorage is single-threaded — no concurrency primitives available.
//
// Platform adaptation is transparent via Cell2<T>:
//   Mutex<T> on native, RefCell<T> on wasm.
//   Same borrow()/borrow_mut() API regardless of platform.
//
// All public storage methods are async. Sync callers use
// futures_executor::block_on externally (see FihBlackboard).

pub mod helper;
pub mod io;
pub mod process;
pub mod storage;

#[cfg(not(target_arch = "wasm32"))]
pub use io::FsIo;
pub use io::{AsyncFileIo, SyncFileIo, WriteOp};
pub use process::{error::ProcessError, scheduler::Scheduler};
pub use storage::core::export::{FihExport, FihImport, export_from_io, import_into_io};
pub use storage::core::{EntityStore, FihSession, FihStorage, IntentStatus, MemoryEntityStore};
pub use storage::fih::FihBlackboard;
pub use storage::semantic::SemanticStore;
pub use storage::semantic::record::{Query, RecordLoad};

/// Shortcut to create a default blackboard (hot petgraph + no cold backend).
pub fn create_blackboard() -> nexus_storage_composite::HybridBlackboard {
    nexus_storage_composite::HybridBlackboard::new()
}
