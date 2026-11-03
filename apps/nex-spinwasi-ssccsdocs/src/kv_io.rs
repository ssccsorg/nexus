// KvIo: Spin KV Store-backed FileIo for Fermyon Cloud persistence.
//
// Each key is prefixed with "fih:" to namespace within the KV store.
// Writes are immediate (no batch buffering), matching MemIo semantics.
// On Fermyon Cloud, data persists across instance restarts.

use spin_sdk::key_value::Store;

use nex::io::{FileIo, IoFuture};

pub struct KvIo {
    store: Store,
}

impl KvIo {
    pub fn new() -> Result<Self, String> {
        let store = Store::open_default().map_err(|e| format!("kv open: {e}"))?;
        Ok(Self { store })
    }
}

impl FileIo for KvIo {
    fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        let key = format!("fih:{}", path);
        let result = self.store.get(&key);
        Box::pin(async move {
            match result {
                Ok(Some(data)) => Ok(Some(data)),
                Ok(None) => Ok(None),
                Err(e) => Err(format!("kv read {path}: {e}")),
            }
        })
    }

    fn write<'a>(&'a self, path: &'a str, data: &'a [u8]) -> IoFuture<'a, ()> {
        let key = format!("fih:{}", path);
        let data = data.to_vec();
        Box::pin(async move {
            self.store
                .set(&key, &data)
                .map_err(|e| format!("kv write {path}: {e}"))?;
            Ok(())
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> IoFuture<'a, Vec<String>> {
        let kv_prefix = format!("fih:{}", prefix);
        Box::pin(async move {
            let keys = self.store.get_keys().map_err(|e| format!("kv list: {e}"))?;
            let fih_prefix = "fih:";
            Ok(keys
                .into_iter()
                .filter(|k| k.starts_with(&kv_prefix))
                .map(|k| k.strip_prefix(fih_prefix).unwrap_or(&k).to_string())
                .collect())
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> IoFuture<'a, ()> {
        let key = format!("fih:{}", path);
        Box::pin(async move {
            self.store
                .delete(&key)
                .map_err(|e| format!("kv delete {path}: {e}"))?;
            Ok(())
        })
    }
}
