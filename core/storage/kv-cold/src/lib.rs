// nexus-storage-kv-cold — Platform-independent KV + Blob cold storage for FIH.
//
// Provides KeyValueStore and BlobStore traits, plus KvColdStorage which
// implements the full ColdStorage trait (FihPersistence + FilterCapable +
// ScanCapable + TimeRangeCapable + FlushCapable + CypherCapable) using
// any K: KeyValueStore + B: BlobStore pair.
//
// This is the primary cold storage for WASM/CF Workers environments where
// DuckDB (C bindings) cannot run. It also works for local development and
// dedicated servers with alternative K/B implementations.

pub mod kv_cold;
pub mod mock;

// Re-export traits and main type.
pub use kv_cold::KvColdStorage;
pub use mock::{MockBlob, MockKv};

/// Simple key-value store abstraction.
///
/// Implementations: MockKv (test), worker::kv::Namespace (CF Workers),
/// sled (server), HashMap (embedded).
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
/// Implementations: MockBlob (test), R2 bucket (CF Workers),
/// local filesystem (server).
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
