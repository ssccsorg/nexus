// ── Async storage traits: async counterparts of FactCapable, StorageRead, etc.
//
// These traits mirror the sync versions but use `async fn` (AFIT, Rust 1.75+).
// Backends that implement these can be used directly in async contexts
// (CF Workers, tokio, wasm-bindgen) without `futures_executor::block_on`.
//
// The sync traits (`FactCapable`, `StorageRead`, etc.) remain unchanged for
// native/blocking use. A backend implements whichever set fits its runtime.

use crate::error::BlackboardError;
use crate::fih::{BoardState, Fact, FihHash, Hint, Intent};

/// Async counterpart of [`super::read::StorageRead`].
pub trait AsyncStorageRead {
    fn project_id(&self) -> &str;
    async fn read_state(&self) -> BoardState;
}

/// Async counterpart of [`super::fact::FactCapable`].
pub trait AsyncFactCapable: AsyncStorageRead {
    async fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError>;
}

/// Async counterpart of [`super::hint::HintCapable`].
pub trait AsyncHintCapable: AsyncStorageRead {
    async fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError>;
}

/// Async counterpart of [`super::intent::IntentCapable`].
pub trait AsyncIntentCapable: AsyncStorageRead {
    async fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError>;
    async fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError>;
    async fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError>;
    async fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError>;
    async fn conclude_intent(&self, intent_id: &str, result: &str) -> Result<crate::fih::Fact, BlackboardError>;
}
