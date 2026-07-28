// Mock implementations for in-memory KeyValueStore, BlobStore, and ObjectStore.
//
// Each integration test binary compiles this module but may not use every mock.
// The `dead_code` allow is intentional: this is a shared mock library.
#![allow(dead_code)]

use nexus_model::{BlobStore, MetaStore, ObjectStore};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// In-memory KeyValueStore backed by a HashMap.
#[derive(Debug, Clone)]
pub struct MockKv {
    data: Arc<RwLock<HashMap<String, String>>>,
}

impl MockKv {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for MockKv {
    fn default() -> Self {
        Self::new()
    }
}

impl MetaStore for MockKv {
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

/// In-memory BlobStore backed by a HashMap<Vec<u8>>.
#[derive(Debug, Clone)]
pub struct MockBlob {
    data: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl MockBlob {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for MockBlob {
    fn default() -> Self {
        Self::new()
    }
}

impl BlobStore for MockBlob {
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

/// In-memory ObjectStore backed by Arc<RwLock<HashMap<String, String>>>.
#[derive(Debug, Clone)]
pub struct MockObject {
    data: Arc<RwLock<HashMap<String, String>>>,
}

impl MockObject {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for MockObject {
    fn default() -> Self {
        Self::new()
    }
}

impl ObjectStore for MockObject {
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
