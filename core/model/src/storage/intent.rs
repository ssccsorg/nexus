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
