/// Atomic CAS store for cross-worker coordination.
///
/// Implementations: IoBufferObject (in-memory HashMap),
/// Durable Object (CF Workers), Redis lock (server).
///
/// Each key represents an independent CAS namespace. In CF Workers,
/// `key` maps to a DO instance ID, making each Intent claim its own
/// atomic gate.
pub trait ObjectStore: Send + Sync {
    /// Read current state for a key. Returns None if key does not exist.
    fn get_state(&self, key: &str) -> Result<Option<String>, String>;

    /// Compare-and-swap: atomically set `key` to `new` only if current
    /// value matches `expected`. Returns true if the swap succeeded.
    fn put_state(&self, key: &str, expected: &str, new: &str) -> Result<bool, String>;
}
