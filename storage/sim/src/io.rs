// ── AsyncFihIo: async storage IO abstraction ───────────────────────────
//
// The single IO boundary. Every storage backend (local fs, remote storage,
// SQLite, in-memory) implements this trait. The core never calls IO directly.
//
// Async trait. Sync callers use BlockingFihIo wrapper.

use std::future::Future;
use std::pin::Pin;

/// A single storage operation that can be committed or rolled back.
/// The core enqueues these; FihSession flushes them as a batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteOp {
    /// Write a record file: path -> bytes.
    Write { path: String, data: Vec<u8> },
    /// Delete a single file.
    Delete { path: String },
}

/// Async IO operations on a flat key-space.
///
/// The key-space is flat (`facts/f_{hash}.fact`, `blob/{hash}.bin`).
/// Directory structure is an implementation detail of the IO layer.
#[allow(clippy::type_complexity)]
pub trait AsyncFihIo {
    /// Read a single file. Returns None if not found.
    fn read<'a>(
        &'a self,
        path: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Vec<u8>>, String>> + 'a>>;

    /// Write a single file. Creates parent directories if needed.
    fn write<'a>(
        &'a self,
        path: &'a str,
        data: &'a [u8],
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + 'a>>;

    /// List all paths with the given prefix.
    fn list<'a>(
        &'a self,
        prefix: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>, String>> + 'a>>;

    /// Delete a single file. Ok if not found.
    fn delete<'a>(
        &'a self,
        path: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + 'a>>;

    /// Apply a batch of WriteOps. Default impl calls write/delete sequentially.
    fn apply_batch<'a>(
        &'a self,
        ops: &'a [WriteOp],
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + 'a>> {
        Box::pin(async move {
            for op in ops {
                match op {
                    WriteOp::Write { path, data } => self.write(path, data).await?,
                    WriteOp::Delete { path } => self.delete(path).await?,
                }
            }
            Ok(())
        })
    }
}

/// Wraps an AsyncFihIo into a blocking/sync FihIo interface.
/// Uses futures_executor::block_on internally.
pub struct BlockingFihIo<A: AsyncFihIo> {
    inner: A,
}

impl<A: AsyncFihIo> BlockingFihIo<A> {
    pub fn new(inner: A) -> Self {
        Self { inner }
    }

    pub fn read(&self, path: &str) -> Result<Option<Vec<u8>>, String> {
        futures_executor::block_on(self.inner.read(path))
    }

    pub fn write(&self, path: &str, data: &[u8]) -> Result<(), String> {
        futures_executor::block_on(self.inner.write(path, data))
    }

    pub fn list(&self, prefix: &str) -> Result<Vec<String>, String> {
        futures_executor::block_on(self.inner.list(prefix))
    }

    pub fn delete(&self, path: &str) -> Result<(), String> {
        futures_executor::block_on(self.inner.delete(path))
    }

    pub fn apply_batch(&self, ops: &[WriteOp]) -> Result<(), String> {
        futures_executor::block_on(self.inner.apply_batch(ops))
    }
}
