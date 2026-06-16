// ── NativeBlackboard: FihStorage-backed Blackboard implementation ──────
//
// A drop-in replacement for DefaultBlackboard that uses FihStorage
// instead of PetgraphStorage + DualStorage. No petgraph dependency.
//
// Feature-gated behind `native` feature.
//
// On native: uses SimIo (in-memory) for testing.
// On CF Workers: use CfFihIo (R2-backed) via `cf` feature.
//
// Usage:
//   let bb = NativeBlackboard::new("project");
//   bb.submit_fact(&fact).unwrap();
//   let state = bb.read_state();

use nexus_model::{
    BlackboardError, BoardState, EvictCapable, Fact, FactCapable, FihHash, FlushCapable,
    FlushCursor, FlushResult, Hint, HintCapable, Intent, IntentCapable, PartitionData, ScanCapable,
    StorageRead,
};

use crate::io::AsyncFileIo;
use crate::storage::core::FihStorage;
use nexus_storage_sim::SimIo;

/// Blackboard implementation backed by FihStorage.
///
/// On native (default): uses SimIo (in-memory HashMap, no persistence).
/// On CF (`feature = "cf"`): uses CfFihIo (R2-backed persistence).
///
/// All Blackboard trait methods delegate directly to FihStorage.
/// No petgraph, no DualStorage, no claims tracker (FihStorage handles it).
pub struct NativeBlackboard<I: AsyncFileIo> {
    pub storage: FihStorage<I>,
}

impl NativeBlackboard<SimIo> {
    /// Create a new in-memory NativeBlackboard (SimIo-backed).
    /// Auto-flush is enabled for standalone use (not FihSession).
    pub fn new(project_id: &str) -> Self {
        Self {
            storage: FihStorage::with_auto_flush(SimIo::new(), project_id),
        }
    }
}

impl<I: AsyncFileIo> StorageRead for NativeBlackboard<I> {
    fn project_id(&self) -> &str {
        self.storage.project_id()
    }

    fn read_state(&self) -> BoardState {
        self.storage.read_state()
    }
}

impl<I: AsyncFileIo> FactCapable for NativeBlackboard<I> {
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        self.storage.submit_fact(fact)
    }
}

impl<I: AsyncFileIo> HintCapable for NativeBlackboard<I> {
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        self.storage.submit_hint(hint)
    }
}

impl<I: AsyncFileIo> IntentCapable for NativeBlackboard<I> {
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

impl<I: AsyncFileIo> EvictCapable for NativeBlackboard<I> {
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

impl<I: AsyncFileIo> FlushCapable for NativeBlackboard<I> {
    fn flush_since(&self, cursor: &FlushCursor) -> Result<FlushResult, String> {
        self.storage.flush_since(cursor)
    }
}

impl<I: AsyncFileIo> ScanCapable for NativeBlackboard<I> {
    fn scan_partition(&self, partition: &str) -> Result<PartitionData, String> {
        self.storage.scan_partition(partition)
    }
}

impl<I: AsyncFileIo> NativeBlackboard<I> {
    /// Rebuild in-memory cache from IO storage. Call on cold start
    /// to restore previous state from persistent backend (R2, fs, etc.).
    ///
    /// Panics on `wasm32-unknown-unknown` where `block_on` is
    /// unavailable. Use the async methods directly on `FihStorage`
    /// when targeting WASM.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn rebuild_cache(&self) -> Result<(), String> {
        futures_executor::block_on(self.storage.rebuild_cache())
    }

    /// Flush pending writes to IO storage. Call after each write
    /// operation to ensure durability.
    ///
    /// Panics on `wasm32-unknown-unknown` where `block_on` is
    /// unavailable. Use the async methods directly on `FihStorage`
    /// when targeting WASM.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn flush_pending(&self) -> Result<(), String> {
        futures_executor::block_on(self.storage.flush_pending())
    }
}
