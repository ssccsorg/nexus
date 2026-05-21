use super::aggregate::{ColdStorage, HotStorage};
use super::fact::FactCapable;
use super::filter::{FilterCapable, StateFilter};
use super::hint::HintCapable;
use super::intent::IntentCapable;
use super::read::StorageRead;
use crate::error::BlackboardError;
use crate::fih::{BoardState, Fact, FihHash, Hint, Intent};

/// Composes a Hot + Cold storage pair.
///
/// - Writes go to both hot and cold (dual-write for durability).
/// - Reads go to hot (early return, edge computing fast path).
/// - Flush/evict delegate to the appropriate layer.
pub struct DualStorage {
    hot: Box<dyn HotStorage>,
    cold: Box<dyn ColdStorage>,
}

impl DualStorage {
    pub fn new(hot: Box<dyn HotStorage>, cold: Box<dyn ColdStorage>) -> Self {
        Self { hot, cold }
    }

    pub fn hot(&self) -> &dyn HotStorage {
        &*self.hot
    }

    pub fn cold(&self) -> &dyn ColdStorage {
        &*self.cold
    }
}

// ── Core read ──

impl StorageRead for DualStorage {
    fn project_id(&self) -> &str {
        self.hot.project_id()
    }

    fn read_state(&self) -> BoardState {
        self.hot.read_state()
    }
}

// ── FIH writes: delegate to both hot + cold ──

impl FactCapable for DualStorage {
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        let hash = self.hot.submit_fact(fact)?;
        self.cold.submit_fact(fact)?;
        Ok(hash)
    }
}

impl IntentCapable for DualStorage {
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        let hash = self.hot.submit_intent(intent)?;
        self.cold.submit_intent(intent)?;
        Ok(hash)
    }

    fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.hot.claim_intent(intent_id, agent)?;
        self.cold.claim_intent(intent_id, agent)?;
        Ok(())
    }

    fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.hot.heartbeat(intent_id, agent)?;
        self.cold.heartbeat(intent_id, agent)?;
        Ok(())
    }

    fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.hot.release_intent(intent_id, agent)?;
        self.cold.release_intent(intent_id, agent)?;
        Ok(())
    }

    fn conclude_intent(
        &self,
        intent_id: &str,
        result: &serde_json::Value,
    ) -> Result<Fact, BlackboardError> {
        let fact = self.hot.conclude_intent(intent_id, result)?;
        self.cold.conclude_intent(intent_id, result)?;
        Ok(fact)
    }
}

impl HintCapable for DualStorage {
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        self.hot.submit_hint(hint)?;
        self.cold.submit_hint(hint)?;
        Ok(())
    }
}

// ── Filtered reads: delegate to cold (hot typically doesn't support filtering) ──

impl FilterCapable for DualStorage {
    fn read_state_filtered(&self, filter: &StateFilter) -> BoardState {
        self.cold.read_state_filtered(filter)
    }
}
