// nexus-storage-composite — Platform-independent multi-tier cold storage for FIH.
//
// Provides KeyValueStore, BlobStore, and ObjectStore traits, plus CompositeColdStorage
// which implements the full ColdStorage trait by orchestrating three tiers:
//
//   Tier 1 (KV)     — fast single-key r/w, recent buffer, cursor persistence
//   Tier 2 (Blob)   — JSON-lines archive, bulk scan, flush target (R2, S3)
//   Tier 3 (Object) — CAS-based coordination, snapshot ownership (Durable Object, future)
//
// External bindings (rs-worker, CF Workers) inject concrete K/B/O implementations.
// CompositeColdStorage itself is fully platform-independent.

pub mod composite;
pub mod iobuf;
pub mod session_server;
pub mod store_session;

pub use composite::CompositeColdStorage;
pub use iobuf::{IoBufferBlob, IoBufferKv, IoBufferObject};
pub use session_server::{SessionHandle, SessionServer};
pub use store_session::IoBufferSession;

// Now trait and SystemClock are defined directly in this module (see below).
// composite.rs accesses them via `use crate::{Now, SystemClock}`.

/// Simple key-value store abstraction.
///
/// Implementations: IoBufferKv (production, in-memory HashMap),
/// worker::kv::Namespace (CF Workers), sled (server), MockKv (test).
pub trait KeyValueStore: Send + Sync {
    /// Get a value by key. Returns None if not found.
    fn get(&self, key: &str) -> Result<Option<String>, String>;

    /// Set a value. Overwrites if key exists.
    fn set(&self, key: &str, value: &str) -> Result<(), String>;

    /// Delete a key. Ok if key does not exist.
    fn delete(&self, key: &str) -> Result<(), String>;

    /// List all keys with the given prefix.
    fn list(&self, prefix: &str) -> Result<Vec<String>, String>;
}

/// Blob store for Parquet chunks and other binary data.
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

/// KV key for a fact.
pub(crate) fn fact_key(project_id: &str, fact_id: &str) -> String {
    format!("{project_id}:fact:{fact_id}")
}

/// KV key for an intent.
pub(crate) fn intent_key(project_id: &str, intent_id: &str) -> String {
    format!("{project_id}:intent:{intent_id}")
}

/// KV key for a hint.
pub(crate) fn hint_key(project_id: &str, hint_id: &str) -> String {
    format!("{project_id}:hint:{hint_id}")
}

/// KV key for the flush cursor.
pub(crate) fn cursor_key(project_id: &str) -> String {
    format!("{project_id}:cursor")
}

/// Blob key for a JSON-lines file produced by a flush.
pub(crate) fn flush_blob_key(project_id: &str, entity: &str, partition: &str, ts: &str) -> String {
    format!("{project_id}/flush/{entity}/{partition}/{ts}.jsonl")
}

/// Blob key prefix listing all flush blobs for a project+entity+partition.
pub(crate) fn flush_blob_prefix(project_id: &str, entity: &str, partition: &str) -> String {
    format!("{project_id}/flush/{entity}/{partition}/")
}

/// KV prefix for listing all facts in a project.
pub(crate) fn fact_prefix(project_id: &str) -> String {
    format!("{project_id}:fact:")
}

/// KV prefix for listing all intents in a project.
pub(crate) fn intent_prefix(project_id: &str) -> String {
    format!("{project_id}:intent:")
}

/// KV prefix for listing all hints in a project.
pub(crate) fn hint_prefix(project_id: &str) -> String {
    format!("{project_id}:hint:")
}
