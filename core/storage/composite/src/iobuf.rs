// IoBuffer* — in-memory I/O proxies for K/V/Blob/Object with dirty tracking.
//
// These implement KeyValueStore, BlobStore, and ObjectStore with plain
// HashMap storage. Each tracks dirty keys so a StoreSession can flush
// only changed entries to the remote source of truth (CF KV, R2, DO).
//
// These are NOT test mocks. They are production components that act as
// the working copy between the async CF bindings layer and the sync
// CompositeColdStorage.

use crate::{BlobStore, KeyValueStore, ObjectStore};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

// ── IoBufferKv ──────────────────────────────────────────────────────────

/// In-memory key-value store with dirty tracking.
///
/// Each `set`/`delete` marks the key as dirty. After `StoreSession::flush_delta`,
/// the caller clears dirty sets. Thread-safe via `Arc<RwLock<...>>`.
///
/// **Lock ordering**: `data` must always be acquired before `dirty_puts` or
/// `dirty_deletes`. This matches the order used in all trait impl methods
/// (`set()`, `delete()`), avoiding the classic lock-ordering deadlock.
#[derive(Debug, Clone)]
pub struct IoBufferKv {
    data: Arc<RwLock<HashMap<String, String>>>,
    dirty_puts: Arc<RwLock<HashSet<String>>>,
    dirty_deletes: Arc<RwLock<HashSet<String>>>,
}

impl IoBufferKv {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
            dirty_puts: Arc::new(RwLock::new(HashSet::new())),
            dirty_deletes: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Hydrate the buffer from a key-value iterator (e.g. CF KV list response).
    /// Existing data is not cleared — call only once per StoreSession.
    pub fn hydrate_batch(&self, entries: impl IntoIterator<Item = (String, String)>) {
        let mut map = self
            .data
            .write()
            .expect("IoBufferKv hydrate: lock poisoned");
        for (k, v) in entries {
            map.insert(k, v);
        }
    }

    /// Drain and return all dirty puts (changed/added keys → value).
    ///
    /// Lock order: data first, then dirty_puts. Must match set().
    pub fn drain_dirty_puts(&self) -> Vec<(String, String)> {
        let map = self.data.read().unwrap();
        let mut keys = self.dirty_puts.write().unwrap();
        let result: Vec<_> = keys
            .drain()
            .filter_map(|k| map.get(&k).map(|v| (k, v.clone())))
            .collect();
        result
    }

    /// Drain and return all dirty deletes (removed keys).
    pub fn drain_dirty_deletes(&self) -> Vec<String> {
        self.dirty_deletes.write().unwrap().drain().collect()
    }
}

impl Default for IoBufferKv {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyValueStore for IoBufferKv {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        let map = self.data.read().map_err(|e| e.to_string())?;
        Ok(map.get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        let mut map = self.data.write().map_err(|e| e.to_string())?;
        map.insert(key.to_string(), value.to_string());
        self.dirty_puts
            .write()
            .map_err(|e| e.to_string())?
            .insert(key.to_string());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        let mut map = self.data.write().map_err(|e| e.to_string())?;
        map.remove(key);
        self.dirty_deletes
            .write()
            .map_err(|e| e.to_string())?
            .insert(key.to_string());
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

// ── IoBufferBlob ────────────────────────────────────────────────────────

/// In-memory blob store with dirty tracking.
///
/// Blobs are stored as `Vec<u8>`. Dirty tracking: `dirty_puts` contains
/// keys that were written; `dirty_deletes` contains keys that were removed.
#[derive(Debug, Clone)]
pub struct IoBufferBlob {
    data: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    dirty_puts: Arc<RwLock<HashSet<String>>>,
    dirty_deletes: Arc<RwLock<HashSet<String>>>,
}

impl IoBufferBlob {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
            dirty_puts: Arc::new(RwLock::new(HashSet::new())),
            dirty_deletes: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Hydrate the buffer from a key-value iterator.
    pub fn hydrate_batch(&self, entries: impl IntoIterator<Item = (String, Vec<u8>)>) {
        let mut map = self
            .data
            .write()
            .expect("IoBufferBlob hydrate: lock poisoned");
        for (k, v) in entries {
            map.insert(k, v);
        }
    }

    /// Drain dirty puts: (key, data).
    ///
    /// Lock order: data first, then dirty_puts. Must match put().
    pub fn drain_dirty_puts(&self) -> Vec<(String, Vec<u8>)> {
        let map = self.data.read().unwrap();
        let mut keys = self.dirty_puts.write().unwrap();
        let result: Vec<_> = keys
            .drain()
            .filter_map(|k| map.get(&k).map(|v| (k, v.clone())))
            .collect();
        result
    }

    /// Drain dirty deletes: keys that were removed.
    pub fn drain_dirty_deletes(&self) -> Vec<String> {
        self.dirty_deletes.write().unwrap().drain().collect()
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
        self.dirty_puts
            .write()
            .map_err(|e| e.to_string())?
            .insert(key.to_string());
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        let map = self.data.read().map_err(|e| e.to_string())?;
        Ok(map.get(key).cloned())
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        let mut map = self.data.write().map_err(|e| e.to_string())?;
        map.remove(key);
        self.dirty_deletes
            .write()
            .map_err(|e| e.to_string())?
            .insert(key.to_string());
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

/// In-memory CAS store with dirty tracking.
///
/// Each key is an independent CAS namespace. `dirty_puts` tracks keys
/// touched by `put_state`; empty-string writes count as deletes.
#[derive(Debug, Clone)]
pub struct IoBufferObject {
    data: Arc<RwLock<HashMap<String, String>>>,
    dirty_puts: Arc<RwLock<HashSet<String>>>,
    dirty_deletes: Arc<RwLock<HashSet<String>>>,
}

impl IoBufferObject {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
            dirty_puts: Arc::new(RwLock::new(HashSet::new())),
            dirty_deletes: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Hydrate the buffer from key-value pairs.
    pub fn hydrate_batch(&self, entries: impl IntoIterator<Item = (String, String)>) {
        let mut map = self
            .data
            .write()
            .expect("IoBufferObject hydrate: lock poisoned");
        for (k, v) in entries {
            map.insert(k, v);
        }
    }

    /// Drain dirty puts: CAS keys → values.
    ///
    /// Lock order: data first, then dirty_puts. Must match put_state().
    pub fn drain_dirty_puts(&self) -> Vec<(String, String)> {
        let map = self.data.read().unwrap();
        let mut keys = self.dirty_puts.write().unwrap();
        let result: Vec<_> = keys
            .drain()
            .filter_map(|k| map.get(&k).map(|v| (k, v.clone())))
            .collect();
        result
    }

    /// Drain dirty deletes: keys that were removed via put_state("").
    pub fn drain_dirty_deletes(&self) -> Vec<String> {
        self.dirty_deletes.write().unwrap().drain().collect()
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
                self.dirty_deletes
                    .write()
                    .map_err(|e| e.to_string())?
                    .insert(key.to_string());
            } else {
                map.insert(key.to_string(), new.to_string());
                self.dirty_puts
                    .write()
                    .map_err(|e| e.to_string())?
                    .insert(key.to_string());
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
