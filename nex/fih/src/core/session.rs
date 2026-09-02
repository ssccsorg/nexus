// ── FihSession: hydrate/buffer/flush session for FihStorage ──────
//
// Wraps FihStorage<I> and provides the hydrate/flush lifecycle that
// StoreSession used to provide for CompositeColdStorage.
//
// Unlike StoreSession, FihSession is generic over any FihIo implementation
// and does not require separate MetaStore/BlobStore/ObjectStore instances.
//
// FihSession is used by storage/sim (native-only verification runner).
// It uses block_on internally to wrap FihStorage's async methods into
// a synchronous interface for convenience. This is acceptable because
// storage/sim targets native platforms where block_on does not hang.
//
// The synchronous wrapper requires the `std` feature: `block_on` needs
// an executor, which needs std. On no_std targets (MCU) the caller
// drives FihStorage's async methods directly from the launcher's own
// executor (e.g. embassy).

#[cfg(feature = "std")]
use crate::core::store::FihStorage;
#[cfg(feature = "std")]
use crate::io::file_io::FileIo;

#[cfg(feature = "std")]
use alloc::string::String;

#[cfg(feature = "std")]
use futures_executor::block_on;

/// Session wrapper around FihStorage that manages the
/// hydrate → (read/write) → flush lifecycle.
#[cfg(feature = "std")]
pub struct FihSession<I: FileIo> {
    pub storage: FihStorage<I>,
    flushed: bool,
}

#[cfg(feature = "std")]
impl<I: FileIo> FihSession<I> {
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
