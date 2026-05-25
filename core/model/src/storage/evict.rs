use super::read::StorageRead;

/// Backend supports memory management / eviction (hot layer).
pub trait EvictCapable: StorageRead {
    fn approximate_size(&self) -> usize;
    fn evict_before(&self, before: &str) -> Result<u64, String>;

    /// Evict intents that are NOT concluded and older than `older_than_secs`.
    ///
    /// Unlike `evict_before` (which targets concluded/stale-claimed intents),
    /// this targets unclaimed, unconcluded intents that have accumulated
    /// from stigmergy detectors (gap, contradiction, state-change).
    ///
    /// Default: no-op. Cold storage typically doesn't need this;
    /// implementations that do (PetgraphStorage) override.
    fn evict_stale_intents(&self, older_than_secs: u64) -> Result<u64, String> {
        let _ = older_than_secs;
        Ok(0)
    }
}
