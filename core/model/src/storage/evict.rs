use super::read::StorageRead;

/// Backend supports memory management / eviction (hot layer).
pub trait EvictCapable: StorageRead {
    fn approximate_size(&self) -> usize;
    fn evict_before(&self, before: &str) -> Result<u64, String>;

    /// Evict intents that are neither claimed nor concluded, and older
    /// than `older_than_secs`. These intents accumulated from automated
    /// observers and were never acted upon by any agent.
    ///
    /// Default: no-op. Cold storage typically doesn't need this;
    /// implementations that do (PetgraphStorage) override.
    fn evict_stale_intents(&self, older_than_secs: u64) -> Result<u64, String> {
        let _ = older_than_secs;
        Ok(0)
    }
}
