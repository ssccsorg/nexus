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

/// Shortcut to create a default blackboard (hot petgraph + no cold backend).
pub fn create_blackboard() -> nexus_storage_composite::HybridBlackboard {
    nexus_storage_composite::HybridBlackboard::new()
}
