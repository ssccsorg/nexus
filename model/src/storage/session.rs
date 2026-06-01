// Session capability traits — bridge async I/O to sync storage execution.
//
// Follows the same fine-grained capability pattern used by
// StorageRead, FactCapable, etc.
//
//   SessionExecute — access the underlying ColdStorage for sync ops
//
// Implementations: IoBufferSession (composite), future adapters.

use super::ColdStorage;

/// Provides access to the underlying ColdStorage for sync orchestration.
pub trait SessionExecute {
    type Storage: ColdStorage;

    fn storage(&self) -> &Self::Storage;
}
