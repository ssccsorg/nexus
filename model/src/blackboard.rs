use crate::error::BlackboardError;
use crate::fih::{BoardState, Fact, FihHash, Hint, Intent};

// ── Blackboard trait — FIH lifecycle (public, stable) ─────────────────────

pub trait Blackboard {
    fn project_id(&self) -> &str {
        "default"
    }

    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError>;
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError>;
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError>;
    fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError>;
    fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError>;
    fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError>;
    fn conclude_intent(&self, intent_id: &str, result: &str) -> Result<Fact, BlackboardError>;
    fn read_state(&self) -> BoardState;
}

impl<T: Blackboard> Blackboard for &T {
    fn project_id(&self) -> &str {
        (**self).project_id()
    }
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        (**self).submit_fact(fact)
    }
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        (**self).submit_hint(hint)
    }
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
    fn read_state(&self) -> BoardState {
        (**self).read_state()
    }
}

impl<T: Blackboard> Blackboard for &mut T {
    fn project_id(&self) -> &str {
        (**self).project_id()
    }
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        (**self).submit_fact(fact)
    }
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        (**self).submit_hint(hint)
    }
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
    fn read_state(&self) -> BoardState {
        (**self).read_state()
    }
}
