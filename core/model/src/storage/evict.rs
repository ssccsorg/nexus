use super::read::StorageRead;

/// Backend supports memory management / eviction (hot layer).
pub trait EvictCapable: StorageRead {
    fn approximate_size(&self) -> usize;
    fn evict_before(&self, before: &str) -> Result<u64, String>;
}
