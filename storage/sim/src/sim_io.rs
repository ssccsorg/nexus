// ── SimFihIo: in-memory deterministic IO implementation ─────────────────
//
// Backed by HashMap. All operations are O(1) and synchronous.
// Supports failure injection for scenario testing.
// Compatible with wasm32-unknown-unknown (no std::fs dependency).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::io::{AsyncFihIo, IoFuture};

/// Deterministic in-memory IO. No filesystem, no network, no async.
/// On wasm32, uses Rc<RefCell<>> internally; on native, Arc<RwLock<>>.
pub struct SimFihIo {
    data: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    /// If set, every N-th write will fail. 0 = no failures.
    failure_rate: f64,
    /// Monotonic operation counter for failure injection.
    op_count: Arc<RwLock<u64>>,
}

impl SimFihIo {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
            failure_rate: 0.0,
            op_count: Arc::new(RwLock::new(0)),
        }
    }

    /// Set failure injection rate. 0.1 = 10% of writes fail.
    pub fn with_failure_rate(mut self, rate: f64) -> Self {
        self.failure_rate = rate.clamp(0.0, 1.0);
        self
    }

    /// Reset state. Used between test scenarios.
    pub fn clear(&self) {
        let mut map = self.data.write().unwrap();
        map.clear();
        let mut count = self.op_count.write().unwrap();
        *count = 0;
    }

    /// Count of files stored.
    pub fn len(&self) -> usize {
        self.data.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.read().unwrap().is_empty()
    }
}

impl Clone for SimFihIo {
    fn clone(&self) -> Self {
        Self {
            data: Arc::clone(&self.data),
            failure_rate: self.failure_rate,
            op_count: Arc::clone(&self.op_count),
        }
    }
}

impl Default for SimFihIo {
    fn default() -> Self {
        Self::new()
    }
}

impl AsyncFihIo for SimFihIo {
    fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        Box::pin(async move {
            let map = self.data.read().map_err(|e| e.to_string())?;
            Ok(map.get(path).cloned())
        })
    }

    fn write<'a>(&'a self, path: &'a str, data: &'a [u8]) -> IoFuture<'a, ()> {
        Box::pin(async move {
            // Failure injection
            if self.failure_rate > 0.0 {
                let mut count = self.op_count.write().map_err(|e| e.to_string())?;
                *count += 1;
                let should_fail = (*count as f64 * self.failure_rate).fract() < self.failure_rate;
                if should_fail {
                    return Err(format!("injected failure on op {}", *count));
                }
            }

            let mut map = self.data.write().map_err(|e| e.to_string())?;
            map.insert(path.to_string(), data.to_vec());
            Ok(())
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> IoFuture<'a, Vec<String>> {
        Box::pin(async move {
            let map = self.data.read().map_err(|e| e.to_string())?;
            let mut keys: Vec<String> = map
                .keys()
                .filter(|k| k.starts_with(prefix))
                .cloned()
                .collect();
            keys.sort();
            Ok(keys)
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> IoFuture<'a, ()> {
        Box::pin(async move {
            let mut map = self.data.write().map_err(|e| e.to_string())?;
            map.remove(path);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::BlockingFihIo;

    #[test]
    fn test_write_read_roundtrip() {
        let io = SimFihIo::new();
        let blocking = BlockingFihIo::new(io);
        blocking.write("facts/f_test.fact", b"hello").unwrap();
        let data = blocking
            .read("facts/f_test.fact")
            .unwrap()
            .expect("should exist");
        assert_eq!(data, b"hello");
    }

    #[test]
    fn test_read_nonexistent() {
        let io = SimFihIo::new();
        let blocking = BlockingFihIo::new(io);
        assert!(blocking.read("nonexistent").unwrap().is_none());
    }

    #[test]
    fn test_delete() {
        let io = SimFihIo::new();
        let blocking = BlockingFihIo::new(io);
        blocking.write("facts/f_test.fact", b"data").unwrap();
        blocking.delete("facts/f_test.fact").unwrap();
        assert!(blocking.read("facts/f_test.fact").unwrap().is_none());
    }

    #[test]
    fn test_list_prefix() {
        let io = SimFihIo::new();
        let blocking = BlockingFihIo::new(io);
        blocking.write("facts/f_a.fact", b"a").unwrap();
        blocking.write("facts/f_b.fact", b"b").unwrap();
        blocking.write("blob/hash.bin", b"c").unwrap();
        let facts = blocking.list("facts/").unwrap();
        assert_eq!(facts.len(), 2);
        assert!(facts.contains(&"facts/f_a.fact".to_string()));
        assert!(facts.contains(&"facts/f_b.fact".to_string()));
    }

    #[test]
    fn test_failure_injection() {
        let io = SimFihIo::new().with_failure_rate(1.0); // 100% fail
        let blocking = BlockingFihIo::new(io);
        assert!(blocking.write("x", b"data").is_err());
    }

    #[test]
    fn test_clear() {
        let io = SimFihIo::new();
        let blocking = BlockingFihIo::new(io.clone());
        blocking.write("test", b"data").unwrap();
        assert_eq!(io.len(), 1);
        io.clear();
        assert_eq!(io.len(), 0);
    }
}
