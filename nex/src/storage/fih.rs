// ── FihBlackboard: FihStorage-backed Blackboard implementation ──────
//
// A production Blackboard implementation backed by FihStorage.
// Generic over any AsyncFileIo implementation (CfFihIo for R2,
// FsIo for filesystem, SimIo for in-memory testing).
//
// Usage:
//   let bb = FihBlackboard::new(io, "project");

use nexus_model::{
    BlackboardError, BoardState, EvictCapable, Fact, FactCapable, FihHash, FlushCapable,
    FlushCursor, FlushResult, Hint, HintCapable, Intent, IntentCapable, PartitionData, ScanCapable,
    StorageRead,
};

use crate::io::AsyncFileIo;
use crate::storage::core::FihStorage;

/// Blackboard implementation backed by FihStorage.
///
/// Generic over any AsyncFileIo implementation. The IO backend is
/// injected at construction time (e.g., `CfFihIo` for R2, `FsIo` for
/// filesystem, `SimIo` for in-memory).
pub struct FihBlackboard<I: AsyncFileIo> {
    pub storage: FihStorage<I>,
}

impl<I: AsyncFileIo> FihBlackboard<I> {
    /// Create a new FihBlackboard with the given IO backend.
    /// Use FihStorage::with_auto_flush for immediate durability.
    pub fn new(io: I, project_id: &str) -> Self {
        Self {
            storage: FihStorage::new(io, project_id),
        }
    }
}

impl<I: AsyncFileIo> StorageRead for FihBlackboard<I> {
    fn project_id(&self) -> &str {
        self.storage.project_id()
    }
    fn read_state(&self) -> BoardState {
        self.storage.read_state()
    }
}

impl<I: AsyncFileIo> FactCapable for FihBlackboard<I> {
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        self.storage.submit_fact(fact)
    }
}

impl<I: AsyncFileIo> HintCapable for FihBlackboard<I> {
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        self.storage.submit_hint(hint)
    }
}

impl<I: AsyncFileIo> IntentCapable for FihBlackboard<I> {
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        self.storage.submit_intent(intent)
    }
    fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.storage.claim_intent(intent_id, agent)
    }
    fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.storage.heartbeat(intent_id, agent)
    }
    fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.storage.release_intent(intent_id, agent)
    }
    fn conclude_intent(&self, intent_id: &str, result: &str) -> Result<Fact, BlackboardError> {
        self.storage.conclude_intent(intent_id, result)
    }
}

impl<I: AsyncFileIo> EvictCapable for FihBlackboard<I> {
    fn approximate_size(&self) -> usize {
        self.storage.approximate_size()
    }
    fn evict_before(&self, before: &str) -> Result<u64, String> {
        self.storage.evict_before(before)
    }
    fn evict_stale_intents(&self, older_than_secs: u64) -> Result<u64, String> {
        self.storage.evict_stale_intents(older_than_secs)
    }
}

impl<I: AsyncFileIo> FlushCapable for FihBlackboard<I> {
    fn flush_since(&self, cursor: &FlushCursor) -> Result<FlushResult, String> {
        self.storage.flush_since(cursor)
    }
}

impl<I: AsyncFileIo> ScanCapable for FihBlackboard<I> {
    fn scan_partition(&self, partition: &str) -> Result<PartitionData, String> {
        self.storage.scan_partition(partition)
    }
}

impl<I: AsyncFileIo> FihBlackboard<I> {
    /// Rebuild in-memory cache from IO storage. Call on cold start.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn rebuild_cache(&self) -> Result<(), String> {
        futures_executor::block_on(self.storage.rebuild_cache())
    }

    /// Flush pending writes to IO storage.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn flush_pending(&self) -> Result<(), String> {
        futures_executor::block_on(self.storage.flush_pending())
    }
}
