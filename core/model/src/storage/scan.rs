use super::read::StorageRead;
use crate::fih::{Fact, Hint, Intent};

/// Data returned from a partition scan.
#[derive(Debug, Clone)]
pub struct PartitionData {
    pub partition: String,
    pub facts: Vec<Fact>,
    pub intents: Vec<Intent>,
    pub hints: Vec<Hint>,
}

/// Backend supports partition-based bulk scanning (for large-scale analysis).
pub trait ScanCapable: StorageRead {
    fn scan_partition(&self, partition: &str) -> Result<PartitionData, String>;
}
