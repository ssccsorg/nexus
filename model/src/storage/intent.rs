use super::read::StorageRead;
use crate::error::BlackboardError;
use crate::fih::{Fact, FihHash, Intent};

/// Backend can manage Intents (full lifecycle).
pub trait IntentCapable: StorageRead {
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError>;
    fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError>;
    fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError>;
    fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError>;
    fn conclude_intent(&self, intent_id: &str, result: &str) -> Result<Fact, BlackboardError>;
}

impl<T: IntentCapable> IntentCapable for &T {
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        (**self).submit_intent(intent)
    }
    fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        (**self).claim_intent(intent_id, agent)
    }
    fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        (**self).heartbeat(intent_id, agent)
    }
    fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        (**self).release_intent(intent_id, agent)
    }
    fn conclude_intent(&self, intent_id: &str, result: &str) -> Result<Fact, BlackboardError> {
        (**self).conclude_intent(intent_id, result)
    }
}

impl<T: IntentCapable> IntentCapable for &mut T {
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        (**self).submit_intent(intent)
    }
    fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        (**self).claim_intent(intent_id, agent)
    }
    fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        (**self).heartbeat(intent_id, agent)
    }
    fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        (**self).release_intent(intent_id, agent)
    }
    fn conclude_intent(&self, intent_id: &str, result: &str) -> Result<Fact, BlackboardError> {
        (**self).conclude_intent(intent_id, result)
    }
}
