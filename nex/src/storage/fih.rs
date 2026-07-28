// ── FihBlackboard: Optional sync wrapper over FihStorage ────────────
//
// FihBlackboard wraps FihStorage and implements sync Blackboard traits
// (StorageRead, FactCapable, etc.) by calling
// futures_executor::block_on internally.
//
// This exists only for legacy consumers that require a synchronous
// interface on native platforms. It is NOT the recommended interface
// for new code. The sync wrapper lives at this layer, not in
// FihStorage itself, because FihStorage is an async-only execution
// unit. Adding sync trait impls to FihStorage would imply that
// blocking on I/O is acceptable, when in fact it would stall the
// sole thread and starve all pending operations.
//
// Usage (native only):
//   let bb = FihBlackboard::new(io, "project");

use crate::io::FileIo;
use crate::storage::core::FihStorage;
// Async* traits are needed for method resolution on FihStorage in non-WASM impl blocks.
// The wasm32 check flags them as unused (impl blocks are cfg-gated).
#[cfg_attr(target_arch = "wasm32", allow(unused_imports))]
use nexus_model::{
    AsyncEvictCapable, AsyncFactCapable, AsyncHintCapable, AsyncIntentCapable, AsyncStorageRead,
    BlackboardError, BoardState, EvictCapable, Fact, FactCapable, FihHash, Hint, HintCapable,
    Intent, IntentCapable, StorageRead,
};

/// Blackboard implementation backed by FihStorage.
///
/// Generic over any FileIo implementation. The IO backend is
/// injected at construction time (e.g., `CfFihIo` for R2, `FsIo` for
/// filesystem, `SimIo` for in-memory).
pub struct FihBlackboard<I: FileIo> {
    pub storage: FihStorage<I>,
}

impl<I: FileIo> FihBlackboard<I> {
    /// Create a new FihBlackboard with the given IO backend.
    /// Use FihStorage::with_auto_flush for immediate durability.
    pub fn new(io: I, project_id: &str) -> Self {
        Self {
            storage: FihStorage::new(io, project_id),
        }
    }
}

impl<I: FileIo> FihBlackboard<I> {
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

// ── Sync trait implementations (native only, block_on) ────────────────

#[cfg(not(target_arch = "wasm32"))]
impl<I: FileIo> StorageRead for FihBlackboard<I> {
    fn project_id(&self) -> &str {
        self.storage.project_id()
    }

    fn read_state(&self) -> BoardState {
        futures_executor::block_on(self.storage.read_state())
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl<I: FileIo> FactCapable for FihBlackboard<I> {
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        futures_executor::block_on(self.storage.submit_fact(fact))
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl<I: FileIo> IntentCapable for FihBlackboard<I> {
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        futures_executor::block_on(self.storage.submit_intent(intent))
    }

    fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        futures_executor::block_on(self.storage.claim_intent(intent_id, agent))
    }

    fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        futures_executor::block_on(self.storage.heartbeat(intent_id, agent))
    }

    fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        futures_executor::block_on(self.storage.release_intent(intent_id, agent))
    }

    fn conclude_intent(&self, intent_id: &str, result: &str) -> Result<Fact, BlackboardError> {
        futures_executor::block_on(self.storage.conclude_intent(intent_id, result))
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl<I: FileIo> HintCapable for FihBlackboard<I> {
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        futures_executor::block_on(self.storage.submit_hint(hint))
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl<I: FileIo> EvictCapable for FihBlackboard<I> {
    fn approximate_size(&self) -> usize {
        futures_executor::block_on(self.storage.approximate_size())
    }

    fn evict_before(&self, before: &str) -> Result<u64, String> {
        futures_executor::block_on(self.storage.evict_before(before))
    }

    fn evict_stale_intents(&self, older_than_secs: u64) -> Result<u64, String> {
        futures_executor::block_on(self.storage.evict_stale_intents(older_than_secs))
    }
}
