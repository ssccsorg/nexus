// AsyncStore* — in-memory I/O proxies for K/V/Blob/Object.
//
// These implement KeyValueStore, BlobStore, and ObjectStore with plain
// HashMap storage. Thread-safe via Arc<RwLock<...>>.
//
// These are NOT test mocks. They are production components that act as
// the working copy between the async CF bindings layer and the sync
// CompositeColdStorage. The async bridge handles hydrate/drain externally.
//
// === CQRS commit channel ===
//
// CQRS separation is achieved by physical instance isolation, NOT by a
// `track_dirty` flag. `AsyncStoreSession` owns two pairs:
//
//   kv / blob                  — general read/write (tracked for drain)
//   commit_kv / commit_blob    — flush output only (separate HashMap)
//
// Consumer drain calls `kv_buf().list()` and `blob_buf().list()`, which
// naturally exclude commit channel data because commit_kv and commit_blob
// are completely independent HashMap instances. No flag is needed.
//
// This differs from the original design which had a `track_dirty` field.
// Physical isolation is simpler, equally safe, and doesn't require every
// drain implementation to check a flag.

use nexus_model::{BlobStore, MetaStore, ObjectStore};
use std::collections::HashMap;

// ── AsyncStoreSessionMeta ──────────────────────────────────────────────────

/// In-memory MetaStore for AsyncStoreSession.
/// Stores cursor position and snapshot pointers.
/// `AsyncStoreSessionMeta` is a newtype over `AsyncStoreKv` providing the `MetaStore`
/// trait implementation. It ensures type-level separation from general Kv usage.
#[derive(Debug, Clone)]
pub struct AsyncStoreSessionMeta(AsyncStoreKv);

impl AsyncStoreSessionMeta {
    pub fn new() -> Self {
        Self(AsyncStoreKv::new())
    }
}

impl Default for AsyncStoreSessionMeta {
    fn default() -> Self {
        Self::new()
    }
}

impl MetaStore for AsyncStoreSessionMeta {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        self.0.get(key)
    }
    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.0.set(key, value)
    }
}

use std::sync::{Arc, RwLock};

// ── AsyncStoreKv ──────────────────────────────────────────────────────────

/// In-memory key-value store. Thread-safe via `Arc<RwLock<...>>`.
///
/// Pure HashMap storage. Physical instance isolation provides CQRS separation:
/// general buffers and commit-channel buffers are independent HashMap instances.
#[derive(Debug, Clone)]
pub struct AsyncStoreKv {
    data: Arc<RwLock<HashMap<String, String>>>,
}

impl AsyncStoreKv {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Bulk-load entries (pre-hydration).
    pub fn hydrate_batch(&self, entries: impl IntoIterator<Item = (String, String)>) {
        let mut map = self.data.write().unwrap();
        for (k, v) in entries {
            map.insert(k, v);
        }
    }
}

impl Default for AsyncStoreKv {
    fn default() -> Self {
        Self::new()
    }
}

impl MetaStore for AsyncStoreKv {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        let map = self.data.read().map_err(|e| e.to_string())?;
        Ok(map.get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        let mut map = self.data.write().map_err(|e| e.to_string())?;
        map.insert(key.to_string(), value.to_string());
        Ok(())
    }
}

// ── AsyncStoreBlob ────────────────────────────────────────────────────────

/// In-memory blob store. Thread-safe via `Arc<RwLock<...>>`.
///
/// Pure HashMap storage. Physical instance isolation provides CQRS separation.
#[derive(Debug, Clone)]
pub struct AsyncStoreBlob {
    data: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl AsyncStoreBlob {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Bulk-load entries (pre-hydration).
    pub fn hydrate_batch(&self, entries: impl IntoIterator<Item = (String, Vec<u8>)>) {
        let mut map = self.data.write().unwrap();
        for (k, v) in entries {
            map.insert(k, v);
        }
    }
}

impl Default for AsyncStoreBlob {
    fn default() -> Self {
        Self::new()
    }
}

impl BlobStore for AsyncStoreBlob {
    fn put(&self, key: &str, data: &[u8]) -> Result<(), String> {
        let mut map = self.data.write().map_err(|e| e.to_string())?;
        map.insert(key.to_string(), data.to_vec());
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        let map = self.data.read().map_err(|e| e.to_string())?;
        Ok(map.get(key).cloned())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        let mut map = self.data.write().map_err(|e| e.to_string())?;
        map.remove(key);
        Ok(())
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, String> {
        let map = self.data.read().map_err(|e| e.to_string())?;
        let mut keys: Vec<String> = map
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        keys.sort();
        Ok(keys)
    }
}

// ── AsyncStoreObject ──────────────────────────────────────────────────────

/// In-memory CAS store. Each key is an independent CAS namespace.
///
/// ObjectStore does not participate in CQRS commit channel separation —
/// CAS operations are inherently isolated and never bulk-drained.
#[derive(Debug, Clone)]
pub struct AsyncStoreObject {
    data: Arc<RwLock<HashMap<String, String>>>,
}

impl AsyncStoreObject {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Bulk-load entries without dirty tracking (pre-hydration).
    pub fn hydrate_batch(&self, entries: impl IntoIterator<Item = (String, String)>) {
        let mut map = self.data.write().unwrap();
        for (k, v) in entries {
            map.insert(k, v);
        }
    }
}

impl Default for AsyncStoreObject {
    fn default() -> Self {
        Self::new()
    }
}

impl ObjectStore for AsyncStoreObject {
    fn get_state(&self, key: &str) -> Result<Option<String>, String> {
        let map = self.data.read().map_err(|e| e.to_string())?;
        Ok(map.get(key).cloned())
    }

    fn put_state(&self, key: &str, expected: &str, new: &str) -> Result<bool, String> {
        let mut map = self.data.write().map_err(|e| e.to_string())?;
        let current = map.get(key).map(|s| s.as_str()).unwrap_or("");
        if current == expected {
            if new.is_empty() {
                map.remove(key);
            } else {
                map.insert(key.to_string(), new.to_string());
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
