// AsyncStoreSession — concrete session backed by AsyncStoreBlob/Object + MetaStore.
//
// Owns a MetaStore + BlobStore + ObjectStore trio + CompositeColdStorage.
// Implements SessionExecute from nexus-model.
//
// Architecture:
//
//   CF KV / R2 / DO  (source of truth, async)
//       ↓ hydrate_*       ↑ cursor-driven flush
//   AsyncStoreSession         │
//   ├── AsyncStoreBlob        │  ← pure HashMap (BlobStore)
//   ├── AsyncStoreObject      │  ← pure HashMap (ObjectStore)
//   ├── AsyncStoreMeta        │  ← pure HashMap (MetaStore, cursor/delta ptrs)
//   └── CompositeColdStorage (sync) ← orchestration
//       ├── blob (archive)
//       ├── object (CAS coordination)
//       └── meta (cursor position, snapshot pointers)
//
// The consumer (CF Worker, blockchain validator, etc.):
//   1. Creates an AsyncStoreSession
//   2. Hydrates AsyncStore* from external source (async)
//   3. Runs CompositeColdStorage sync operations via storage()
//   4. Reads cursor via read_cursor() to determine flush boundary
//
// CompositeColdStorage itself is pure sync, never touches async code.
// AsyncStoreSession is pure sync, never touches external I/O.

use nexus_model::SessionExecute;

use super::cold::CompositeColdStorage;
use super::{AsyncStoreBlob, AsyncStoreObject, AsyncStoreSessionMeta};
use nexus_model::SystemClock;

/// Session backed by AsyncStore* + CompositeColdStorage.
///
/// Implements `SessionExecute` (access to ColdStorage).
/// The async bridge layer handles hydrate/flush;
/// AsyncStoreSession provides the sync surface.
pub struct AsyncStoreSession {
    storage: CompositeColdStorage<AsyncStoreBlob, AsyncStoreObject, AsyncStoreSessionMeta, SystemClock>,
    /// Meta buffer (cursor, snapshot pointers). Excluded from drain.
    meta_buf: AsyncStoreSessionMeta,
}

impl AsyncStoreSession {
    /// Create a fresh session for the given project.
    ///
    /// All AsyncStore* instances start empty. The caller must call
    /// `hydrate_*` methods before executing storage operations.
    pub fn new(project_id: impl Into<String>) -> Self {
        let blob = AsyncStoreBlob::new();
        let object = AsyncStoreObject::new();
        let meta = AsyncStoreSessionMeta::new();
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
    pub fn meta_buf(&self) -> &AsyncStoreSessionMeta {
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
    pub fn blob_buf(&self) -> &AsyncStoreBlob {
        self.storage.blob()
    }

    /// Access the Object buffer directly for low-level operations.
    pub fn object_buf(&self) -> &AsyncStoreObject {
        self.storage.object()
    }
}

// ── SessionExecute ───────────────────────────────────────────────────────

impl SessionExecute for AsyncStoreSession {
    type Storage =
        CompositeColdStorage<AsyncStoreBlob, AsyncStoreObject, AsyncStoreSessionMeta, SystemClock>;

    fn storage(&self) -> &Self::Storage {
        &self.storage
    }
}
