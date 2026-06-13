// ── FihSession: hydrate/buffer/flush session for FihStorage ──────
//
// Wraps FihStorage<I> and provides the hydrate/flush lifecycle that
// StoreSession used to provide for CompositeColdStorage.
//
// Unlike StoreSession, FihSession is generic over any FihIo implementation
// and does not require separate MetaStore/BlobStore/ObjectStore instances.

use crate::io::AsyncFihIo;
use crate::store::FihStorage;

/// Session wrapper around FihStorage that manages the
/// hydrate → (read/write) → flush lifecycle.
pub struct FihSession<I: AsyncFihIo> {
    pub storage: FihStorage<I>,
    flushed: bool,
}

impl<I: AsyncFihIo> FihSession<I> {
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
        self.storage.rebuild_cache()?;
        self.flushed = true;
        Ok(())
    }

    /// Flush: write all pending WriteOps to IO.
    /// After flush, the storage is in sync with IO.
    pub fn flush(&mut self) -> Result<(), String> {
        self.storage.flush_pending()?;
        self.flushed = true;
        Ok(())
    }

    /// Has the session been flushed since the last write?
    pub fn is_flushed(&self) -> bool {
        self.flushed && self.storage.pending.lock().unwrap().is_empty()
    }

    /// Access the underlying storage for FIH operations.
    pub fn storage(&self) -> &FihStorage<I> {
        &self.storage
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim_io::SimFihIo;
    use nexus_model::{Content, Fact, FactCapable, FihHash, StorageRead};

    #[test]
    fn test_session_hydrate_flush() {
        let io = SimFihIo::new();
        let mut session = FihSession::new(io.clone(), "test");

        // Write a fact via storage
        let fact = Fact {
            id: FihHash("f001".into()),
            origin: "test".into(),
            content: Content {
                mime_type: "text/plain".into(),
                data: b"hello".to_vec(),
            },
            creator: "alice".into(),
        };
        session.storage.submit_fact(&fact).unwrap();

        // Not yet flushed → data is in buffer, not in IO
        assert!(!session.is_flushed());

        // Flush → data goes to IO
        session.flush().unwrap();
        assert!(session.is_flushed());

        // Read back from a fresh session on the same IO instance
        let mut session2 = FihSession::new(io, "test");
        session2.hydrate().unwrap();
        let state = session2.storage.read_state();
        assert_eq!(state.facts.len(), 1);
        assert_eq!(state.facts[0].id.0, "f001");
    }
}
