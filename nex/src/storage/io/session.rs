// ── FihSession: hydrate/buffer/flush session for FihStorage ──────
//
// Wraps FihStorage<I> and provides the hydrate/flush lifecycle that
// StoreSession used to provide for CompositeColdStorage.
//
// Unlike StoreSession, FihSession is generic over any FihIo implementation
// and does not require separate MetaStore/BlobStore/ObjectStore instances.

use super::async_file_io::AsyncFileIo;
use super::store::FihStorage;
use futures_executor::block_on;

/// Session wrapper around FihStorage that manages the
/// hydrate → (read/write) → flush lifecycle.
pub struct FihSession<I: AsyncFileIo> {
    pub storage: FihStorage<I>,
    flushed: bool,
}

impl<I: AsyncFileIo> FihSession<I> {
    /// Create a new session. Storage is empty until hydrate() or
    /// operations are called.
    pub fn new(io: I, project_id: &str) -> Self {
        Self {
            storage: FihStorage::new(io, project_id),
            flushed: true,
        }
    }

    /// Hydrate: rebuild in-memory cache from IO storage.
    /// Call this after constructor to load existing data.
    pub fn hydrate(&mut self) -> Result<(), String> {
        block_on(self.storage.rebuild_cache())?;
        self.flushed = true;
        Ok(())
    }

    /// Flush: write all pending WriteOps to IO.
    /// After flush, the storage is in sync with IO.
    pub fn flush(&mut self) -> Result<(), String> {
        block_on(self.storage.flush_pending())?;
        self.flushed = true;
        Ok(())
    }

    /// Has the session been flushed since the last write?
    pub fn is_flushed(&self) -> bool {
        self.flushed && self.storage.pending.borrow().is_empty()
    }

    /// Access the underlying storage for FIH operations.
    pub fn storage(&self) -> &FihStorage<I> {
        &self.storage
    }
}
