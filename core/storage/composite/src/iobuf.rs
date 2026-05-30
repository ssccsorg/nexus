// IoBuffer* — in-memory I/O proxies for K/V/Blob/Object.
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
// `track_dirty` flag. `IoBufferSession` owns two pairs:
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

use crate::{BlobStore, MetaStore, ObjectStore};
use std::collections::HashMap;

// ── IoBufferSessionMeta ──────────────────────────────────────────────────

/// In-memory MetaStore for IoBufferSession.
/// Stores cursor position and snapshot pointers.
/// `IoBufferSessionMeta` is a newtype over `IoBufferKv` providing the `MetaStore`
/// trait implementation. It ensures type-level separation from general Kv usage.
#[derive(Debug, Clone)]
pub struct IoBufferSessionMeta(IoBufferKv);

impl IoBufferSessionMeta {
    pub fn new() -> Self {
        Self(IoBufferKv::new())
    }
}

impl Default for IoBufferSessionMeta {
    fn default() -> Self {
        Self::new()
    }
}

impl MetaStore for IoBufferSessionMeta {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        self.0.get(key)
    }
    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.0.set(key, value)
    }
}

use std::sync::{Arc, RwLock};

// ── IoBufferKv ──────────────────────────────────────────────────────────

/// In-memory key-value store. Thread-safe via `Arc<RwLock<...>>`.
///
/// Pure HashMap storage. Physical instance isolation provides CQRS separation:
/// general buffers and commit-channel buffers are independent HashMap instances.
#[derive(Debug, Clone)]
pub struct IoBufferKv {
    data: Arc<RwLock<HashMap<String, String>>>,
}

impl IoBufferKv {
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

impl Default for IoBufferKv {
    fn default() -> Self {
        Self::new()
    }
}

impl MetaStore for IoBufferKv {
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

// ── IoBufferBlob ────────────────────────────────────────────────────────

/// In-memory blob store. Thread-safe via `Arc<RwLock<...>>`.
///
/// Pure HashMap storage. Physical instance isolation provides CQRS separation.
#[derive(Debug, Clone)]
pub struct IoBufferBlob {
    data: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl IoBufferBlob {
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

impl Default for IoBufferBlob {
    fn default() -> Self {
        Self::new()
    }
}

impl BlobStore for IoBufferBlob {
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

// ── IoBufferObject ──────────────────────────────────────────────────────

/// In-memory CAS store. Each key is an independent CAS namespace.
///
/// ObjectStore does not participate in CQRS commit channel separation —
/// CAS operations are inherently isolated and never bulk-drained.
#[derive(Debug, Clone)]
pub struct IoBufferObject {
    data: Arc<RwLock<HashMap<String, String>>>,
}

impl IoBufferObject {
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

impl Default for IoBufferObject {
    fn default() -> Self {
        Self::new()
    }
}

impl ObjectStore for IoBufferObject {
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
