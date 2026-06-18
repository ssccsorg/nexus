use std::cell::RefCell;
use nex::io::{AsyncFileIo, IoFuture, WriteOp};

/// AsyncFileIo wrapper that batches writes and flushes them in one `apply_batch`.
///
/// Reads, lists, and deletes pass through to the inner IO immediately.
/// Writes are enqueued and only committed when `apply_batch` is called.
/// This reduces R2 PUT requests from N to 1 for bulk operations.
pub struct BatchIo<I: AsyncFileIo> {
    inner: I,
    pending: RefCell<Vec<WriteOp>>,
}

impl<I: AsyncFileIo> BatchIo<I> {
    pub fn new(inner: I) -> Self {
        Self { inner, pending: RefCell::new(Vec::new()) }
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
        self.inner.apply_batch(&batch).await
    }

    pub fn pending_count(&self) -> usize {
        self.pending.borrow().len()
    }
}

impl<I: AsyncFileIo> AsyncFileIo for BatchIo<I> {
    fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        self.inner.read(path)
    }

    fn write<'a>(&'a self, path: &'a str, data: &'a [u8]) -> IoFuture<'a, ()> {
        let p = path.to_string();
        let d = data.to_vec();
        self.pending.borrow_mut().push(WriteOp::Write { path: p, data: d });
        Box::pin(std::future::ready(Ok(())))
    }

    fn list<'a>(&'a self, prefix: &'a str) -> IoFuture<'a, Vec<String>> {
        self.inner.list(prefix)
    }

    fn delete<'a>(&'a self, path: &'a str) -> IoFuture<'a, ()> {
        self.inner.delete(path)
    }

    /// Flush pending writes + forward to inner.
    fn apply_batch<'a>(&'a self, ops: &'a [WriteOp]) -> IoFuture<'a, ()> {
        let this: &BatchIo<I> = self;
        let ops_vec: Vec<WriteOp> = ops.to_vec();
        Box::pin(async move {
            // First flush our own pending
            this.flush().await?;
            // Then forward the incoming batch
            this.inner.apply_batch(&ops_vec).await
        })
    }
}
