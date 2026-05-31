// nexus-storage-composite — Platform-independent multi-tier cold storage for FIH.
//
// Provides BlobStore, ObjectStore, and MetaStore traits, plus CompositeColdStorage
// which implements the durable persistence layer.
//
//   Tier 1 (Blob)   — Petgraph snapshot archive + flush output (R2, S3)
//   Tier 2 (Object) — CAS-based coordination (Durable Object)
//   Tier 3 (Meta)   — Cursor position, snapshot pointers (KV, sled)
//
// Graph CRUD (FactCapable, IntentCapable, HintCapable) is NOT handled here.
// Those are delegated to PetgraphStorage (hot storage) in nexus-storage-petgraph.
//
// External bindings (rs-worker, CF Workers) inject concrete B/O/M implementations.
// CompositeColdStorage itself is fully platform-independent.

pub mod composite;
pub mod iobuf;
pub mod session_server;
pub mod store_session;

pub use composite::CompositeColdStorage;
pub use iobuf::{IoBufferBlob, IoBufferKv, IoBufferObject, IoBufferSessionMeta};
pub use session_server::{SessionHandle, SessionServer};
pub use store_session::IoBufferSession;

// Now trait and SystemClock are defined directly in this module (see below).
// composite.rs accesses them via `use crate::{Now, SystemClock}`.

/// Blob store for Petgraph snapshots, Parquet chunks, and other binary data.
///
/// Implementations: IoBufferBlob (production, in-memory HashMap),
/// R2 bucket (CF Workers), local filesystem (server), MockBlob (test).
pub trait BlobStore: Send + Sync {
    /// Store binary data at the given key.
    fn put(&self, key: &str, data: &[u8]) -> Result<(), String>;

    /// Retrieve binary data by key. Returns None if not found.
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, String>;

    /// Delete a blob. Ok if key does not exist.
    fn delete(&self, key: &str) -> Result<(), String>;

    /// List all blob keys with the given prefix.
    fn list(&self, prefix: &str) -> Result<Vec<String>, String>;
}

/// Atomic CAS store for cross-worker coordination.
///
/// Implementations: IoBufferObject (production, in-memory HashMap),
/// Durable Object stub (CF Workers), Redis lock (server), MockObject (test).
///
/// Each key represents an independent CAS namespace. In CF Workers,
/// `key` maps to a DO instance ID, making each Intent claim its own
/// atomic gate.
pub trait ObjectStore: Send + Sync {
    /// Read current state for a key. Returns None if key does not exist.
    fn get_state(&self, key: &str) -> Result<Option<String>, String>;
    /// Compare-and-swap: atomically set `key` to `new` only if current
    /// value matches `expected`. Returns true if the swap succeeded.
    ///
    /// Usage patterns (all CAS under the hood):
    ///   `put_state(k, "", agent)`     — first claim (expected is empty)
    ///   `put_state(k, old, new)`      — ownership transfer
    ///   `put_state(k, val, "")`       — release (empty = delete)
    fn put_state(&self, key: &str, expected: &str, new: &str) -> Result<bool, String>;
}

/// Minimal key-value store for cursor position, snapshot pointers, and
/// other metadata. NOT for graph data.
///
/// Implementations: IoBufferKv (in-memory HashMap), CF KV Namespace,
/// sled (server), MockKv (test).
///
/// This replaces the earlier general-purpose `KeyValueStore` which was
/// used for graph data storage. MetaStore is intentionally limited to
/// get/set — no list, no delete — because it only stores scalar metadata.
pub trait MetaStore: Send + Sync {
    /// Get a value by key. Returns None if not found.
    fn get(&self, key: &str) -> Result<Option<String>, String>;

    /// Set a value. Overwrites if key exists.
    fn set(&self, key: &str, value: &str) -> Result<(), String>;
}

/// Legacy: backwards-compatible alias for MetaStore.
pub trait KeyValueStore: MetaStore {
    fn list(&self, _prefix: &str) -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }
    fn delete(&self, _key: &str) -> Result<(), String> {
        Ok(())
    }
}

// Auto-impl: any MetaStore is also a KeyValueStore (with no-op list/delete).
impl<T: MetaStore> KeyValueStore for T {}

// ── Now trait ─────────────────────────────────────────────────────────────

/// Clock abstraction for platform-independent timestamp generation.
///
/// Implementations: SystemClock (native), js_sys::Date (WASM).
/// Without this trait, CompositeColdStorage would be hardcoded to SystemTime::now(),
/// which is incorrect for WASM targets and makes testing impossible.
pub trait Now: Send + Sync {
    /// Return current time as a nanosecond-precision string.
    fn now_nanos(&self) -> String;
}

/// SystemTime-based clock. Correct for native targets.
#[derive(Debug, Clone, Copy)]
pub struct SystemClock;

impl Now for SystemClock {
    fn now_nanos(&self) -> String {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_string()
    }
}

// ── Internal key conventions ─────────────────────────────────────────────

/// Blob key prefix listing all flush blobs for a project+entity+partition.
pub(crate) fn flush_blob_prefix(project_id: &str, entity: &str, partition: &str) -> String {
    format!("{project_id}/flush/{entity}/{partition}/")
}
