// ── nex: FIH Blackboard Storage Engine ───────────────────────────────
//
// nex is an execution unit. Each FihStorage instance runs on a single
// thread with exclusive ownership of its in-memory state and I/O
// channel. There is no internal concurrency: no Mutex, no RwLock, no
// thread pool. Scaling happens through physical instance replication,
// not through internal sharding.
//
// All public storage methods are async. FihStorage does NOT implement
// sync storage traits (FactCapable, IntentCapable, etc.). Sync callers
// use futures_executor::block_on externally (see FihBlackboard).
//
// Interior mutability uses RefCell, not Mutex. This is the simplest
// correct implementation for a single-owner model. Thread-safe access
// is achieved by wrapping the instance externally.
//
// No static or static mut state exists except fixed constants. Every
// resource is owned by the instance.

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
