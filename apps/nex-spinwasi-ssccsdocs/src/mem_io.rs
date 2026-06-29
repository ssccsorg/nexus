// MemIo: in-memory HashMap-backed FileIo for Spin WASI.
// No filesystem access needed. Data persists only for the lifetime
// of the WASM instance (typically across multiple requests in Spin).

use std::collections::HashMap;
use std::sync::Mutex;

use nex::io::{FileIo, IoFuture, WriteOp};

/// In-memory IO backend. All reads/writes go to a HashMap.
pub struct MemIo {
    store: Mutex<HashMap<String, Vec<u8>>>,
}

impl MemIo {
    pub fn new() -> Self {
        Self { store: Mutex::new(HashMap::new()) }
    }

    /// No-op flush (writes are immediate).
    pub async fn flush(&self) -> Result<(), String> {
        Ok(())
    }
}

impl FileIo for MemIo {
    fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        let p = path.to_string();
        Box::pin(async move {
            Ok(self.store.lock().unwrap().get(&p).cloned())
        })
    }

    fn write<'a>(&'a self, path: &'a str, data: &'a [u8]) -> IoFuture<'a, ()> {
        let p = path.to_string();
        let d = data.to_vec();
        Box::pin(async move {
            self.store.lock().unwrap().insert(p, d);
            Ok(())
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> IoFuture<'a, Vec<String>> {
        let p = prefix.to_string();
        Box::pin(async move {
            let store = self.store.lock().unwrap();
            Ok(store.keys().filter(|k| k.starts_with(&p)).cloned().collect())
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> IoFuture<'a, ()> {
        let p = path.to_string();
        Box::pin(async move {
            self.store.lock().unwrap().remove(&p);
            Ok(())
        })
    }
}

impl nex::io::BatchIo for MemIo {
    fn apply_batch<'a>(&'a self, ops: &'a [WriteOp]) -> IoFuture<'a, ()> {
        let ops_vec = ops.to_vec();
        Box::pin(async move {
            for op in &ops_vec {
                match op {
                    WriteOp::Write { path, data } => {
                        self.store.lock().unwrap().insert(path.clone(), data.clone());
                    }
                    WriteOp::Delete { path } => {
                        self.store.lock().unwrap().remove(path);
                    }
                }
            }
            Ok(())
        })
    }
}
