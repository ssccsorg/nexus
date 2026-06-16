pub mod blackboard;
pub mod gateway_driver;
pub mod helper;
pub mod io;
pub mod process;
pub mod storage;

// Re-export key types for convenience
pub use blackboard::{DefaultBlackboard, Record};
pub use gateway_driver::GatewayDriver;
pub use io::{AsyncFileIo, SyncFileIo, WriteOp};
pub use nexus_model::{
    Blackboard, BlackboardError, BoardState, Content, EvictCapable, Fact, FactCapable, FihHash,
    FlushCapable, FlushCursor, Hint, HintCapable, Intent, IntentCapable, ScanCapable, StorageRead,
    TimeRangeCapable,
};
pub use process::{error::ProcessError, scheduler::Scheduler};
pub use storage::composite::CompositeColdStorage;
pub use storage::core::export::{FihExport, FihImport, export_from_io, import_into_io};
pub use storage::core::{
    EntityStore, FihSession, FihStorage, IntentStatus, MemoryEntityStore, SystemClock,
};
pub use storage::fih::FihBlackboard;
pub use storage::petgraph::{
    EdgeWeight, GraphRead, GraphWrite, NodeWeight, PetgraphStorage, Snapshottable, StorageSnapshot,
};

/// Create a default blackboard with in-memory petgraph hot storage
/// and no cold backend.
pub fn create_blackboard() -> DefaultBlackboard {
    DefaultBlackboard::new()
}
