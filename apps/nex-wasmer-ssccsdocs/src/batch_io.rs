// BatchIo: write-batching adapter for any FileIo backend.
//
// Reads, lists, and deletes pass through to the inner IO immediately.
// Writes are enqueued and only committed when `apply_batch` is called.
// This reduces IO operations from N to 1 for bulk operations.

use std::sync::Mutex;

use nex::io::{BatchIo as BatchIoTrait, FileIo, IoFuture, WriteOp, default_apply_batch};

/// Write-batching adapter wrapping any FileIo.
pub struct BatchIo<I: FileIo> {
    inner: I,
    pending: Mutex<Vec<WriteOp>>,
}

impl<I: FileIo> BatchIo<I> {
    pub fn new(inner: I) -> Self {
        Self {
            inner,
            pending: Mutex::new(Vec::new()),
        }
    }

    /// Flush pending writes to inner IO via apply_batch.
    pub async fn flush(&self) -> Result<(), String> {
        let batch = {
            let mut ops = self.pending.lock().unwrap();
            if ops.is_empty() {
                return Ok(());
            }
            std::mem::take(&mut *ops)
        };
        default_apply_batch(&self.inner, &batch).await
    }

    /// Returns the number of pending writes (for diagnostics).
    #[expect(dead_code)]
    pub fn pending_count(&self) -> usize {
        self.pending.lock().unwrap().len()
    }
}

impl<I: FileIo> FileIo for BatchIo<I> {
    fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        self.inner.read(path)
    }

    fn write<'a>(&'a self, path: &'a str, data: &'a [u8]) -> IoFuture<'a, ()> {
        let p = path.to_string();
        let d = data.to_vec();
        self.pending
            .lock()
            .unwrap()
            .push(WriteOp::Write { path: p, data: d });
        Box::pin(std::future::ready(Ok(())))
    }

    fn list<'a>(&'a self, prefix: &'a str) -> IoFuture<'a, Vec<String>> {
        self.inner.list(prefix)
    }

    fn delete<'a>(&'a self, path: &'a str) -> IoFuture<'a, ()> {
        self.inner.delete(path)
    }
}

impl<I: FileIo + BatchIoTrait> BatchIoTrait for BatchIo<I> {
    /// Flush pending writes + forward incoming batch to inner.
    fn apply_batch<'a>(&'a self, ops: &'a [WriteOp]) -> IoFuture<'a, ()> {
        let this: &BatchIo<I> = self;
        let ops_vec: Vec<WriteOp> = ops.to_vec();
        Box::pin(async move {
            for op in &ops_vec {
                match op {
                    WriteOp::Write { path, data } => {
                        this.pending.lock().unwrap().push(WriteOp::Write {
                            path: path.clone(),
                            data: data.clone(),
                        });
                    }
                    WriteOp::Delete { path } => {
                        this.pending
                            .lock()
                            .unwrap()
                            .push(WriteOp::Delete { path: path.clone() });
                    }
                }
            }
            this.flush().await
        })
    }
}
