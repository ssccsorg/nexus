// ── AsyncFileIo: flat key-space file IO abstraction ────────────────
//
// The single IO boundary. Every storage backend (local fs, remote object
// store, in-memory HashMap, bare-metal flash) implements this trait.
// The core never calls IO directly.
//
// Despite the name, this trait does NOT require `std::fs` or a local
// filesystem. Implementations include:
//   - SimIo: in-memory HashMap (no_std compatible)
//   - FsIo: std::fs
//   - CfIo: Cloudflare R2 (WASM)
//   - (your backend here): any flat key-space with read/write/list/delete
// # Why async?
//
// Storage is inherently asynchronous. At the hardware level, every I/O
// operation (DRAM read, DMA transfer, NVMe queue, network round-trip)
// involves pipelining, interrupts, or completion queues. None of it is
// truly synchronous. "Sync" is a programmer convenience abstraction over
// cooperative scheduling (async) or preemptive scheduling (OS threads).
//
// By making AsyncFileIo async at the trait level, we align with:
//   - CF Workers: await on R2 bucket.get() directly (no block_on)
//   - tokio: spawn + await on async fs/network
//   - wasm32: single-threaded, cooperative multitasking via await
//
// Sync callers (FihStorage, FactCapable, etc.) use SyncFileIo wrapper,
// which calls futures_executor::block_on internally. This is NOT an adapter
// layer. Async is the design center; sync is the extension.
//
// SyncFileIo does NOT introduce significant overhead because FihStorage
// buffers all writes into pending WriteOps and only flushes in batch.
// Individual IO calls are amortized over the flush window.
//
// AsyncFileIo is the design center. Sync is the extension.

use std::future::Future;
use std::pin::Pin;

/// Type alias to suppress clippy::type_complexity on AsyncFileIo methods.
/// GAT-based alternative (nightly): `type IoFuture<'a, T>: Future<Output = Result<T, String>> + 'a`
/// That would eliminate per-call heap allocation but requires nightly Rust.
/// Keeping Box for now is acceptable for this abstraction layer.
pub type IoFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + 'a>>;

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
pub trait AsyncFileIo {
    /// Read a single file. Returns None if not found.
    fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>>;

    /// Write a single file. Creates parent directories if needed.
    fn write<'a>(&'a self, path: &'a str, data: &'a [u8]) -> IoFuture<'a, ()>;

    /// List all paths with the given prefix.
    fn list<'a>(&'a self, prefix: &'a str) -> IoFuture<'a, Vec<String>>;

    /// Delete a single file. Ok if not found.
    fn delete<'a>(&'a self, path: &'a str) -> IoFuture<'a, ()>;

    /// Apply a batch of WriteOps. Default impl calls write/delete sequentially.
    fn apply_batch<'a>(&'a self, ops: &'a [WriteOp]) -> IoFuture<'a, ()> {
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

/// Wraps an AsyncFileIo into a blocking/sync interface.
/// Uses futures_executor::block_on internally.
pub struct SyncFileIo<A: AsyncFileIo> {
    inner: A,
}

impl<A: AsyncFileIo> SyncFileIo<A> {
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
