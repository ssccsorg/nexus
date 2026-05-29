// StoreSession — owns an IoBuffer trio + CompositeColdStorage for one request lifecycle.
//
// Architecture:
//
//   CF KV / R2 / DO  (source of truth, async)
//       ↓ hydrate       ↑ flush_dirty
//   StoreSession        │
//   ├── IoBufferKv      │  ← dirty tracking
//   ├── IoBufferBlob    │
//   ├── IoBufferObject  │
//   └── CompositeColdStorage (sync) ← orchestration
//
// The WASM entry point:
//   1. Creates a StoreSession
//   2. Hydrates IoBuffer* from CF bindings (async, in rs-worker)
//   3. Runs CompositeColdStorage sync operations via storage()
//   4. Drains dirty data and flushes to CF bindings (async, in rs-worker)
//
// CompositeColdStorage itself is pure sync, never touches async code.
// StoreSession is pure sync, never touches CF bindings.

use crate::composite::CompositeColdStorage;
use crate::{IoBufferBlob, IoBufferKv, IoBufferObject, SystemClock};

/// Holds the complete in-memory working set for a single Worker request.
///
/// Owns IoBuffer trio + CompositeColdStorage. The async bridge (rs-worker)
/// handles all CF I/O; StoreSession provides the sync compute surface.
pub struct StoreSession {
    storage: CompositeColdStorage<IoBufferKv, IoBufferBlob, IoBufferObject, SystemClock>,
}

impl StoreSession {
    /// Create a fresh session for the given project.
    ///
    /// All IoBuffer* instances start empty. The caller must call
    /// `hydrate_kv`, `hydrate_blob`, `hydrate_object` on the
    /// respective buffers before executing storage operations.
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

    /// Access the underlying CompositeColdStorage for sync orchestration.
    pub fn storage(
        &self,
    ) -> &CompositeColdStorage<IoBufferKv, IoBufferBlob, IoBufferObject, SystemClock> {
        &self.storage
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

    // ── Dirty drain surface (called by async bridge after sync execution) ──

    /// Drain dirty KV puts: (key, value) pairs to push to CF KV.
    pub fn drain_kv_puts(&self) -> Vec<(String, String)> {
        self.storage.kv().drain_dirty_puts()
    }

    /// Drain dirty KV deletes: keys to delete from CF KV.
    pub fn drain_kv_deletes(&self) -> Vec<String> {
        self.storage.kv().drain_dirty_deletes()
    }

    /// Drain dirty blob puts: (key, data) pairs to push to R2.
    pub fn drain_blob_puts(&self) -> Vec<(String, Vec<u8>)> {
        self.storage.blob().drain_dirty_puts()
    }

    /// Drain dirty blob deletes: keys to delete from R2.
    pub fn drain_blob_deletes(&self) -> Vec<String> {
        self.storage.blob().drain_dirty_deletes()
    }

    /// Drain dirty object puts: (key, value) pairs to push to DO.
    pub fn drain_object_puts(&self) -> Vec<(String, String)> {
        self.storage.object().drain_dirty_puts()
    }

    /// Drain dirty object deletes: keys to delete from DO.
    pub fn drain_object_deletes(&self) -> Vec<String> {
        self.storage.object().drain_dirty_deletes()
    }
}
