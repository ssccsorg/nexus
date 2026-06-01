use super::read::StorageRead;
use serde::{Deserialize, Serialize};

/// Cursor for incremental flush. Tracks the last flushed position.
///
/// Persisted across scheduler invocations so that `flush_since` exports
/// only data ingested after the last completed flush.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FlushCursor {
    pub last_flushed_at: String,
    pub partition: String,
}

/// Result of a flush operation.
#[derive(Debug, Clone)]
pub struct FlushResult {
    pub records_flushed: u64,
    pub new_cursor: FlushCursor,
}

impl From<FlushResult> for (u64, FlushCursor) {
    fn from(r: FlushResult) -> Self {
        (r.records_flushed, r.new_cursor)
    }
}

/// Backend supports incremental export (flush).
pub trait FlushCapable: StorageRead {
    fn flush_since(&self, cursor: &FlushCursor) -> Result<FlushResult, String>;
}
