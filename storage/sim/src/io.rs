// ── FihIo: storage IO abstraction ───────────────────────────────────────
//
// The single IO boundary. Every storage backend (local fs, remote storage,
// SQLite, in-memory) implements this trait. The core never calls IO directly.
//
// All methods are synchronous. Async buffering is handled by the outer
// FihSession layer, not by this trait.

/// A single storage operation that can be committed or rolled back.
/// The core enqueues these; FihSession flushes them as a batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOp {
    /// Write a record file: path → bytes.
    Write { path: String, data: Vec<u8> },
    /// Delete a single file.
    Delete { path: String },
}

/// Pure IO operations: read, write, list, delete on a flat key-space.
///
/// The key-space is flat (`facts/f_{hash}.fact`, `blob/{hash}.bin`).
/// Directory structure is an implementation detail of the IO layer.
pub trait FihIo {
    /// Read a single file. Returns None if not found.
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, String>;

    /// Write a single file. Creates parent directories if needed.
    fn write(&self, path: &str, data: &[u8]) -> Result<(), String>;

    /// List all paths with the given prefix.
    fn list(&self, prefix: &str) -> Result<Vec<String>, String>;

    /// Delete a single file. Ok if not found.
    fn delete(&self, path: &str) -> Result<(), String>;
}

/// Extension: apply a batch of WriteOps atomically (all or nothing).
pub trait FihIoBatch: FihIo {
    /// Apply a sequence of WriteOps. If any fails, all previous writes
    /// in this batch should be rolled back (implementation-defined).
    fn apply_batch(&self, ops: &[WriteOp]) -> Result<(), String> {
        for op in ops {
            match op {
                WriteOp::Write { path, data } => self.write(path, data)?,
                WriteOp::Delete { path } => self.delete(path)?,
            }
        }
        Ok(())
    }
}

impl<T: FihIo> FihIoBatch for T {}
