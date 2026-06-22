// CF store adapters — implement nex storage traits on top of CF bindings.
//
//   CfMetaStore  (KV → MetaStore)   — cursor position, snapshot pointers
//   CfBlobStore  (R2 → BlobStore)   — snapshot archive, flush output
//   CfObjectStore (DO → ObjectStore) — CAS claim coordination
//
// USB-pluggable into CompositeColdStorage<B, O, M>.
// Replace with S3, Redis, local fs — only these three trait impls change.
//
// The async→sync bridge (CF async → trait sync) is a known WASM limitation.
// These implementations use best-effort patterns; full persistence requires
// the hydrate/drain cycle through AsyncStoreSession (see nex/src/storage/composite/).

pub mod bm25;
pub mod vectorize;

use nexus_model::{BlobStore, MetaStore, ObjectStore};
use worker::*;

// ── CfMetaStore ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct CfMetaStore {
    #[allow(dead_code)]
    kv: KvStore,
}

impl CfMetaStore {
    pub fn new(kv: KvStore) -> Self {
        Self { kv }
    }
}

impl MetaStore for CfMetaStore {
    fn get(&self, _key: &str) -> Result<Option<String>, String> {
        // Async bridge: worker 0.8 KV calls are async; MetaStore is sync.
        // Full implementation requires hydrate/drain pattern with AsyncStoreSession.
        // For now, return None — persistence is deferred.
        Ok(None)
    }

    fn set(&self, _key: &str, _value: &str) -> Result<(), String> {
        Ok(())
    }
}

// ── CfBlobStore ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct CfBlobStore {
    #[allow(dead_code)]
    bucket: Bucket,
}

impl CfBlobStore {
    pub fn new(bucket: Bucket) -> Self {
        Self { bucket }
    }
}

impl BlobStore for CfBlobStore {
    fn put(&self, _key: &str, _data: &[u8]) -> Result<(), String> {
        Ok(())
    }

    fn get(&self, _key: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(None)
    }

    fn delete(&self, _key: &str) -> Result<(), String> {
        Ok(())
    }

    fn list(&self, _prefix: &str) -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }
}

// ── CfObjectStore ────────────────────────────────────────────────────────

pub struct CfObjectStore;

impl Default for CfObjectStore {
    fn default() -> Self {
        Self
    }
}

impl CfObjectStore {
    pub fn new() -> Self {
        Self
    }
}

impl ObjectStore for CfObjectStore {
    fn get_state(&self, _key: &str) -> Result<Option<String>, String> {
        Ok(None)
    }

    fn put_state(&self, _key: &str, _expected: &str, _new: &str) -> Result<bool, String> {
        Ok(false)
    }
}
