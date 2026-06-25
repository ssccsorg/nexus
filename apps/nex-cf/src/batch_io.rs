use nex::io::{BatchIo as BatchIoTrait, FileIo, IoFuture, WriteOp, default_apply_batch};
use std::cell::RefCell;

/// Write-batching adapter wrapping any FileIo.
///
/// Reads, lists, and deletes pass through to the inner IO immediately.
/// Writes are enqueued and only committed when `apply_batch` is called.
/// This reduces R2 PUT requests from N to 1 for bulk operations.
pub struct BatchIo<I: FileIo> {
    inner: I,
    pending: RefCell<Vec<WriteOp>>,
}

impl<I: FileIo> BatchIo<I> {
    pub fn new(inner: I) -> Self {
        Self {
            inner,
            pending: RefCell::new(Vec::new()),
        }
    }

    /// Flush pending writes to inner IO via apply_batch.
    pub async fn flush(&self) -> Result<(), String> {
        let batch = {
            let mut ops = self.pending.borrow_mut();
            if ops.is_empty() {
                return Ok(());
            }
            std::mem::take(&mut *ops)
        };
        default_apply_batch(&self.inner, &batch).await
    }

    pub fn pending_count(&self) -> usize {
        self.pending.borrow().len()
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
            .borrow_mut()
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
                        this.pending.borrow_mut().push(WriteOp::Write {
                            path: path.clone(),
                            data: data.clone(),
                        });
                    }
                    WriteOp::Delete { path } => {
                        this.pending
                            .borrow_mut()
                            .push(WriteOp::Delete { path: path.clone() });
                    }
                }
            }
            // Flush all pending (both original and incoming) in one batch
            this.flush().await
        })
    }
}
