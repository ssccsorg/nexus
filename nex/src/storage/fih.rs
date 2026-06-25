// ── FihBlackboard: Optional sync wrapper over FihStorage ────────────
//
// FihBlackboard wraps FihStorage and implements sync Blackboard traits
// (StorageRead, FactCapable, etc.) by calling
// futures_executor::block_on internally.
//
// This exists only for legacy consumers that require a synchronous
// interface on native platforms. It is NOT the recommended interface
// for new code. The sync wrapper lives at this layer, not in
// FihStorage itself, because FihStorage is an async-only execution
// unit. Adding sync trait impls to FihStorage would imply that
// blocking on I/O is acceptable, when in fact it would stall the
// sole thread and starve all pending operations.
//
// Usage (native only):
//   let bb = FihBlackboard::new(io, "project");

use crate::io::FileIo;
use crate::storage::core::FihStorage;

/// Blackboard implementation backed by FihStorage.
///
/// Generic over any FileIo implementation. The IO backend is
/// injected at construction time (e.g., `CfFihIo` for R2, `FsIo` for
/// filesystem, `SimIo` for in-memory).
pub struct FihBlackboard<I: FileIo> {
    pub storage: FihStorage<I>,
}

impl<I: FileIo> FihBlackboard<I> {
    /// Create a new FihBlackboard with the given IO backend.
    /// Use FihStorage::with_auto_flush for immediate durability.
    pub fn new(io: I, project_id: &str) -> Self {
        Self {
            storage: FihStorage::new(io, project_id),
        }
    }
}

impl<I: FileIo> FihBlackboard<I> {
    /// Rebuild in-memory cache from IO storage. Call on cold start.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn rebuild_cache(&self) -> Result<(), String> {
        futures_executor::block_on(self.storage.rebuild_cache())
    }

    /// Flush pending writes to IO storage.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn flush_pending(&self) -> Result<(), String> {
        futures_executor::block_on(self.storage.flush_pending())
    }
}
