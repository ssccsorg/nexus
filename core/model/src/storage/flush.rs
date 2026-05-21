use super::read::StorageRead;

/// Cursor for incremental flush. Tracks the last flushed position.
#[derive(Debug, Clone)]
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

/// Backend supports incremental export (flush).
pub trait FlushCapable: StorageRead {
    fn flush_since(&self, cursor: &FlushCursor) -> Result<FlushResult, String>;
}
