/// Minimal key-value store for cursor position, snapshot pointers, and
/// other metadata. NOT for graph data.
///
/// Implementations: AsyncStoreKv (in-memory HashMap), CF KV Namespace,
/// sled (server).
///
/// MetaStore is intentionally limited to get/set — no list, no delete —
/// because it only stores scalar metadata.
pub trait MetaStore: Send + Sync {
    /// Get a value by key. Returns None if not found.
    fn get(&self, key: &str) -> Result<Option<String>, String>;

    /// Set a value. Overwrites if key exists.
    fn set(&self, key: &str, value: &str) -> Result<(), String>;
}

/// Legacy: backwards-compatible alias for MetaStore.
pub trait KeyValueStore: MetaStore {
    fn list(&self, _prefix: &str) -> Result<Vec<String>, String> {
        Ok(Vec::new())
    }
    fn delete(&self, _key: &str) -> Result<(), String> {
        Ok(())
    }
}

// Auto-impl: any MetaStore is also a KeyValueStore (with no-op list/delete).
impl<T: MetaStore> KeyValueStore for T {}
