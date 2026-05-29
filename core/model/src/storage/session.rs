// Session capability traits — bridge async I/O to sync storage execution.
//
// Follows the same fine-grained capability + aggregate pattern used by
// StorageRead, FactCapable, etc. Each trait represents one dimension of
// the hydrate → execute → drain lifecycle.
//
//   SessionExecute   — access the underlying ColdStorage for sync ops
//   SessionDrainKv   — drain dirty KV entries
//   SessionDrainBlob — drain dirty blob entries
//   SessionDrainObject — drain dirty object entries
//
// Aggregate: SessionDrain = SessionDrainKv + SessionDrainBlob + SessionDrainObject
// Aggregate: StoreSession  = SessionExecute + SessionDrain
//
// Implementations: IoBufferSession (composite), future adapters.

use super::ColdStorage;

// ── SessionExecute ───────────────────────────────────────────────────────

/// Provides access to the underlying ColdStorage for sync orchestration.
pub trait SessionExecute {
    type Storage: ColdStorage;

    fn storage(&self) -> &Self::Storage;
}

// ── SessionDrain capability traits ───────────────────────────────────────

/// Drain dirty KV entries after sync execution.
pub trait SessionDrainKv {
    fn drain_kv_puts(&self) -> Vec<(String, String)>;
    fn drain_kv_deletes(&self) -> Vec<String>;
}

/// Drain dirty blob entries after sync execution.
pub trait SessionDrainBlob {
    fn drain_blob_puts(&self) -> Vec<(String, Vec<u8>)>;
    fn drain_blob_deletes(&self) -> Vec<String>;
}

/// Drain dirty object entries after sync execution.
pub trait SessionDrainObject {
    fn drain_object_puts(&self) -> Vec<(String, String)>;
    fn drain_object_deletes(&self) -> Vec<String>;
}

// ── Aggregate traits ─────────────────────────────────────────────────────

/// Full session drain capability.
pub trait SessionDrain: SessionDrainKv + SessionDrainBlob + SessionDrainObject {}
impl<T: SessionDrainKv + SessionDrainBlob + SessionDrainObject> SessionDrain for T {}

/// Full session: execute + drain.
pub trait StoreSession: SessionExecute + SessionDrain {}
impl<T: SessionExecute + SessionDrain> StoreSession for T {}
