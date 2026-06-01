// IoBufferSession — concrete session backed by IoBufferBlob/Object + MetaStore.
//
// Owns a MetaStore + BlobStore + ObjectStore trio + CompositeColdStorage.
// Implements SessionExecute from nexus-model.
//
// Architecture:
//
//   CF KV / R2 / DO  (source of truth, async)
//       ↓ hydrate_*       ↑ cursor-driven flush
//   IoBufferSession         │
//   ├── IoBufferBlob        │  ← pure HashMap (BlobStore)
//   ├── IoBufferObject      │  ← pure HashMap (ObjectStore)
//   ├── IoBufferMeta        │  ← pure HashMap (MetaStore, cursor/delta ptrs)
//   └── CompositeColdStorage (sync) ← orchestration
//       ├── blob (archive)
//       ├── object (CAS coordination)
//       └── meta (cursor position, snapshot pointers)
//
// The consumer (CF Worker, blockchain validator, etc.):
//   1. Creates an IoBufferSession
//   2. Hydrates IoBuffer* from external source (async)
//   3. Runs CompositeColdStorage sync operations via storage()
//   4. Reads cursor via read_cursor() to determine flush boundary
//
// CompositeColdStorage itself is pure sync, never touches async code.
// IoBufferSession is pure sync, never touches external I/O.

use nexus_model::SessionExecute;

use super::composite::CompositeColdStorage;
use super::{IoBufferBlob, IoBufferObject, IoBufferSessionMeta};
use nexus_model::SystemClock;

/// Session backed by IoBuffer* + CompositeColdStorage.
///
/// Implements `SessionExecute` (access to ColdStorage).
/// The async bridge layer handles hydrate/flush;
/// IoBufferSession provides the sync surface.
pub struct IoBufferSession {
    storage: CompositeColdStorage<IoBufferBlob, IoBufferObject, IoBufferSessionMeta, SystemClock>,
    /// Meta buffer (cursor, snapshot pointers). Excluded from drain.
    meta_buf: IoBufferSessionMeta,
}

impl IoBufferSession {
    /// Create a fresh session for the given project.
    ///
    /// All IoBuffer* instances start empty. The caller must call
    /// `hydrate_*` methods before executing storage operations.
    pub fn new(project_id: impl Into<String>) -> Self {
        let blob = IoBufferBlob::new();
        let object = IoBufferObject::new();
        let meta = IoBufferSessionMeta::new();
        let storage = CompositeColdStorage::new_with_system_clock(
            blob.clone(),
            object.clone(),
            meta.clone(),
            project_id,
        );
        Self {
            storage,
            meta_buf: meta,
        }
    }

    /// Access the meta store (cursor, snapshot pointers).
    pub fn meta_buf(&self) -> &IoBufferSessionMeta {
        &self.meta_buf
    }

    // ── High-level hydrate API ───────────────────────────────────────────

    /// Preload a batch of raw Blob key-value pairs.
    pub fn hydrate_blob(&self, entries: impl IntoIterator<Item = (String, Vec<u8>)>) {
        self.storage.blob().hydrate_batch(entries);
    }

    /// Preload a batch of raw Object key-value pairs (CAS state).
    pub fn hydrate_object(&self, entries: impl IntoIterator<Item = (String, String)>) {
        self.storage.object().hydrate_batch(entries);
    }

    // ── Low-level buffer access ──────────────────────────────────────────

    /// Access the Blob buffer directly for low-level operations.
    pub fn blob_buf(&self) -> &IoBufferBlob {
        self.storage.blob()
    }

    /// Access the Object buffer directly for low-level operations.
    pub fn object_buf(&self) -> &IoBufferObject {
        self.storage.object()
    }
}

// ── SessionExecute ───────────────────────────────────────────────────────

impl SessionExecute for IoBufferSession {
    type Storage =
        CompositeColdStorage<IoBufferBlob, IoBufferObject, IoBufferSessionMeta, SystemClock>;

    fn storage(&self) -> &Self::Storage {
        &self.storage
    }
}
