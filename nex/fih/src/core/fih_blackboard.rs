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
// The sync wrapper requires the `std` feature: `block_on` needs an
// executor, which needs std. On no_std targets (MCU) the caller
// drives FihStorage's async methods directly from the launcher's own
// executor (e.g. embassy).
//
// Usage (native only):
//   let bb = FihBlackboard::new(io, "project");

#[cfg(feature = "std")]
use crate::core::store::FihStorage;
#[cfg(feature = "std")]
use crate::io::FileIo;
// Async* traits are needed for method resolution on FihStorage in non-WASM impl blocks.
// The wasm32 check flags them as unused (impl blocks are cfg-gated).
#[cfg(feature = "std")]
#[cfg_attr(target_arch = "wasm32", allow(unused_imports))]
use crate::{
    AsyncEvictCapable, AsyncFactCapable, AsyncHintCapable, AsyncIntentCapable, AsyncStorageRead,
    BlackboardError, BoardState, CoordId, EvictCapable, Fact, FactCapable, Hint, HintCapable,
    Intent, IntentCapable, StorageRead,
};

// The sync wrapper methods and trait impls are gated behind the `std`
// feature (they use `block_on`); the `String` in their signatures is
// unused on no_std builds.
#[cfg(feature = "std")]
use alloc::string::String;

/// Blackboard implementation backed by FihStorage.
///
/// Generic over any FileIo implementation. The IO backend is
/// injected at construction time (e.g., `CfFihIo` for R2, `FsIo` for
/// filesystem, `SimIo` for in-memory).
///
/// The synchronous wrapper (all methods) requires the `std` feature:
/// it uses `futures_executor::block_on` internally. The async surface
/// of FihStorage itself is std-free and available on no_std targets.
#[cfg(feature = "std")]
pub struct FihBlackboard<I: FileIo> {
    pub storage: FihStorage<I>,
}

#[cfg(feature = "std")]
impl<I: FileIo> FihBlackboard<I> {
    /// Create a new FihBlackboard with the given IO backend.
    /// Use FihStorage::with_auto_flush for immediate durability.
    pub fn new(io: I, project_id: &str) -> Self {
        Self {
            storage: FihStorage::new(io, project_id),
        }
    }
}

#[cfg(feature = "std")]
impl<I: FileIo> FihBlackboard<I> {
    /// Rebuild in-memory cache from IO storage. Call on cold start.
    pub fn rebuild_cache(&self) -> Result<(), String> {
        futures_executor::block_on(self.storage.rebuild_cache())
    }

    /// Flush pending writes to IO storage.
    pub fn flush_pending(&self) -> Result<(), String> {
        futures_executor::block_on(self.storage.flush_pending())
    }
}

// ── Sync trait implementations (native only, block_on) ────────────────

#[cfg(feature = "std")]
impl<I: FileIo> StorageRead for FihBlackboard<I> {
    fn project_id(&self) -> &str {
        self.storage.project_id()
    }

    fn read_state(&self) -> BoardState {
        futures_executor::block_on(self.storage.read_state())
    }
}

#[cfg(feature = "std")]
impl<I: FileIo> FactCapable for FihBlackboard<I> {
    fn submit_fact(&self, fact: &Fact) -> Result<CoordId, BlackboardError> {
        futures_executor::block_on(self.storage.submit_fact(fact))
    }
}

#[cfg(feature = "std")]
impl<I: FileIo> IntentCapable for FihBlackboard<I> {
    fn submit_intent(&self, intent: &Intent) -> Result<CoordId, BlackboardError> {
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

#[cfg(feature = "std")]
impl<I: FileIo> HintCapable for FihBlackboard<I> {
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        futures_executor::block_on(self.storage.submit_hint(hint))
    }
}

#[cfg(feature = "std")]
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
