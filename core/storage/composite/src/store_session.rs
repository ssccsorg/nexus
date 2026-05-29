// IoBufferSession — concrete session backed by IoBufferKv/Blob/Object.
//
// Owns an IoBuffer trio + CompositeColdStorage. Implements SessionExecute
// from nexus-model.
//
// Architecture:
//
//   CF KV / R2 / DO  (source of truth, async)
//       ↓ hydrate_*       ↑ cursor-driven flush
//   IoBufferSession         │
//   ├── IoBufferKv          │  ← pure HashMap
//   ├── IoBufferBlob        │
//   ├── IoBufferObject      │
//   └── CompositeColdStorage (sync) ← orchestration
//       ├── kv, blob, object (write)
//       ├── commit_kv (cursor state)
//       └── commit_blob (flush archives)
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
use serde::Serialize;

use crate::composite::CompositeColdStorage;
use crate::{IoBufferBlob, IoBufferKv, IoBufferObject, SystemClock};

// ── Stamped envelope helper for hydrate ──────────────────────────────────

/// The internal Stamped envelope that CompositeColdStorage uses.
/// Reproduced here so async consumers can preload raw `Fact`/`Intent`/`Hint`
/// objects without manually constructing JSON.
///
/// Currently unused in production code (tests use their own copy).
/// Future consumers (rs-worker) will use this.
#[allow(dead_code)]
#[derive(Serialize)]
struct Stamped<'a, T: Serialize> {
    submitted_at: &'a str,
    data: &'a T,
}

/// Session backed by IoBuffer* + CompositeColdStorage.
///
/// Implements `SessionExecute` (access to ColdStorage).
/// The async bridge layer handles hydrate/flush;
/// IoBufferSession provides the sync surface.
pub struct IoBufferSession {
    storage: CompositeColdStorage<IoBufferKv, IoBufferBlob, IoBufferObject, SystemClock>,
}

impl IoBufferSession {
    /// Create a fresh session for the given project.
    ///
    /// All IoBuffer* instances start empty. The caller must call
    /// `hydrate_*` methods before executing storage operations.
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

    // ── High-level hydrate API ───────────────────────────────────────────
    //
    // These accept raw `Fact`/`Intent`/`Hint` objects and apply the required
    // Stamped envelope internally. The consumer does not need to know about
    // the internal JSON format.
    //
    // For low-level preload (e.g. from CF KV raw data), use kv_buf()
    // directly with hydrate_batch().

    /// Preload a batch of raw KV key-value pairs without the Stamped envelope.
    /// The consumer is responsible for correct JSON format (raw data from CF KV).
    pub fn hydrate_kv(&self, entries: impl IntoIterator<Item = (String, String)>) {
        self.storage.kv().hydrate_batch(entries);
    }

    /// Preload a batch of raw Blob key-value pairs.
    pub fn hydrate_blob(&self, entries: impl IntoIterator<Item = (String, Vec<u8>)>) {
        self.storage.blob().hydrate_batch(entries);
    }

    /// Preload a batch of raw Object key-value pairs (CAS state).
    pub fn hydrate_object(&self, entries: impl IntoIterator<Item = (String, String)>) {
        self.storage.object().hydrate_batch(entries);
    }

    // ── Low-level buffer access ──────────────────────────────────────────

    /// Access the KV buffer directly for low-level operations.
    pub fn kv_buf(&self) -> &IoBufferKv {
        self.storage.kv()
    }

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
    type Storage = CompositeColdStorage<IoBufferKv, IoBufferBlob, IoBufferObject, SystemClock>;

    fn storage(&self) -> &Self::Storage {
        &self.storage
    }
}
