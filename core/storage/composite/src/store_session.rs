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
//   ├── IoBufferKv          │  ← pure HashMap, track_dirty=true
//   ├── IoBufferBlob        │
//   ├── IoBufferObject      │
//   ├── commit_kv (IoBufferKv, track_dirty=false)  ← CQRS commit channel
//   ├── commit_blob (IoBufferBlob, track_dirty=false)
//   └── CompositeColdStorage (sync) ← orchestration
//       ├── kv, blob, object (write)
//       ├── commit_kv (cursor state — excluded from drain)
//       └── commit_blob (flush archives — excluded from drain)
//
// The consumer (CF Worker, blockchain validator, etc.):
//   1. Creates an IoBufferSession
//   2. Hydrates IoBuffer* from external source (async)
//   3. Runs CompositeColdStorage sync operations via storage()
//   4. Reads cursor via read_cursor() to determine flush boundary
//   5. Drains only IoBuffer* with track_dirty=true — commit channel excluded
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
    /// Commit channel KV (cursor state). `track_dirty=false` — excluded from drain.
    commit_kv: IoBufferKv,
    /// Commit channel Blob (flush archives). `track_dirty=false` — excluded from drain.
    commit_blob: IoBufferBlob,
}

impl IoBufferSession {
    /// Create a fresh session for the given project.
    ///
    /// All IoBuffer* instances start empty. The caller must call
    /// `hydrate_*` methods before executing storage operations.
    ///
    /// === CQRS commit channel ===
    ///
    /// `commit_kv` and `commit_blob` are completely independent IoBuffer
    /// instances (separate HashMap). Consumer drain reads `kv_buf()` and
    /// `blob_buf()` exclusively, so commit channel data is naturally
    /// excluded — no flag checks needed.
    ///
    /// Before this CQRS split, `flush_since()` wrote to `self.blob` and
    /// `self.kv` (same instance), causing a self-referential dirty cycle:
    /// flush output became flush input on the next cycle. Now,
    /// `CompositeColdStorage` writes flush archives to `commit_blob` and
    /// cursor to `commit_kv` — completely separate instances.
    pub fn new(project_id: impl Into<String>) -> Self {
        let kv = IoBufferKv::new();
        let blob = IoBufferBlob::new();
        let object = IoBufferObject::new();
        let commit_kv = IoBufferKv::new();
        let commit_blob = IoBufferBlob::new();
        let storage = CompositeColdStorage::new(
            kv.clone(),
            blob.clone(),
            object.clone(),
            commit_kv.clone(),
            commit_blob.clone(),
            SystemClock,
            project_id,
        );
        Self {
            storage,
            commit_kv,
            commit_blob,
        }
    }

    // ── CQRS accessors ──────────────────────────────────────────────────

    /// Access the commit channel KV (cursor state).
    /// This is a physically separate IoBufferKv instance from the general
    /// buffer. Consumer drain calls `kv_buf()` which never sees this data.
    pub fn commit_kv(&self) -> &IoBufferKv {
        &self.commit_kv
    }

    /// Access the commit channel Blob (flush archives).
    /// Physically separate from `blob_buf()` — excluded from drain naturally.
    pub fn commit_blob(&self) -> &IoBufferBlob {
        &self.commit_blob
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
