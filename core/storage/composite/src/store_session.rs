// IoBufferSession — concrete StoreSession backed by IoBufferKv/Blob/Object.
//
// Owns an IoBuffer trio + CompositeColdStorage. Implements SessionExecute
// and SessionDrain capability traits from nexus-model.
//
// Architecture:
//
//   CF KV / R2 / DO  (source of truth, async)
//       ↓ hydrate_batch     ↑ drain_dirty
//   IoBufferSession         │
//   ├── IoBufferKv          │  ← dirty tracking
//   ├── IoBufferBlob        │
//   ├── IoBufferObject      │
//   └── CompositeColdStorage (sync) ← orchestration
//
// The consumer (CF Worker, blockchain validator, etc.):
//   1. Creates an IoBufferSession
//   2. Hydrates IoBuffer* from external source (async)
//   3. Runs CompositeColdStorage sync operations via storage()
//   4. Drains dirty data and flushes to external source (async)
//
// CompositeColdStorage itself is pure sync, never touches async code.
// IoBufferSession is pure sync, never touches external I/O.

use nexus_model::{
    SessionDrainBlob, SessionDrainKv, SessionDrainObject,
    SessionExecute,
};

use crate::composite::CompositeColdStorage;
use crate::{IoBufferBlob, IoBufferKv, IoBufferObject, SystemClock};

/// Concrete StoreSession with IoBuffer* + CompositeColdStorage.
///
/// Implements `SessionExecute` (access to ColdStorage) and
/// `SessionDrain*` (dirty tracking drain). The async bridge layer
/// handles hydrate/flush; IoBufferSession provides the sync surface.
pub struct IoBufferSession {
    storage: CompositeColdStorage<IoBufferKv, IoBufferBlob, IoBufferObject, SystemClock>,
}

impl IoBufferSession {
    /// Create a fresh session for the given project.
    ///
    /// All IoBuffer* instances start empty. The caller must call
    /// `kv_buf().hydrate_batch(...)` etc. before executing storage operations.
    pub fn new(project_id: impl Into<String>) -> Self {
        let kv = IoBufferKv::new();
        let blob = IoBufferBlob::new();
        let object = IoBufferObject::new();
        let storage = CompositeColdStorage::new_with_system_clock(
            kv.clone(),
            blob.clone(),
            object.clone(),
            project_id,
        );
        Self { storage }
    }

    // ── Hydrate surface (called by async bridge before sync execution) ──

    /// Access the KV buffer for hydrate_batch.
    pub fn kv_buf(&self) -> &IoBufferKv {
        self.storage.kv()
    }

    /// Access the Blob buffer for hydrate_batch.
    pub fn blob_buf(&self) -> &IoBufferBlob {
        self.storage.blob()
    }

    /// Access the Object buffer for hydrate_batch.
    pub fn object_buf(&self) -> &IoBufferObject {
        self.storage.object()
    }
}

// ── SessionExecute ───────────────────────────────────────────────────────

impl SessionExecute for IoBufferSession {
    type Storage = CompositeColdStorage<IoBufferKv, IoBufferBlob, IoBufferObject, SystemClock>;

    fn storage(&self) -> &Self::Storage {
        &self.storage
    }
}

// ── SessionDrainKv ───────────────────────────────────────────────────────

impl SessionDrainKv for IoBufferSession {
    fn drain_kv_puts(&self) -> Vec<(String, String)> {
        self.storage.kv().drain_dirty_puts()
    }

    fn drain_kv_deletes(&self) -> Vec<String> {
        self.storage.kv().drain_dirty_deletes()
    }
}

// ── SessionDrainBlob ─────────────────────────────────────────────────────

impl SessionDrainBlob for IoBufferSession {
    fn drain_blob_puts(&self) -> Vec<(String, Vec<u8>)> {
        self.storage.blob().drain_dirty_puts()
    }

    fn drain_blob_deletes(&self) -> Vec<String> {
        self.storage.blob().drain_dirty_deletes()
    }
}

// ── SessionDrainObject ───────────────────────────────────────────────────

impl SessionDrainObject for IoBufferSession {
    fn drain_object_puts(&self) -> Vec<(String, String)> {
        self.storage.object().drain_dirty_puts()
    }

    fn drain_object_deletes(&self) -> Vec<String> {
        self.storage.object().drain_dirty_deletes()
    }
}
