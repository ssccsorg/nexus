use crate::storage::read::StorageRead;
use alloc::string::String;

/// Backend supports memory management / eviction (hot layer).
pub trait EvictCapable: StorageRead {
    fn approximate_size(&self) -> usize;
    /// Evict hints submitted before `before`, a nanosecond timestamp.
    fn evict_before(&self, before: &str) -> Result<u64, String>;

    /// Evict intents that are neither claimed nor concluded, and older
    /// than `older_than_ns` nanoseconds. These intents accumulated from automated
    /// observers and were never acted upon by any agent.
    ///
    /// Default: no-op. Cold storage typically doesn't need this;
    /// implementations that do (the legacy cold-storage backend) override.
    fn evict_stale_intents(&self, older_than_ns: u64) -> Result<u64, String> {
        let _ = older_than_ns;
        Ok(0)
    }
}
