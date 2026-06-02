/// Mock gateway for transport-layer testing.
///
/// A transparent proxy that round-trips structured FIH types (Fact, Intent,
/// Hint, BoardState) through JSON serialisation before reaching the inner
/// Blackboard. Primitive types (strings, Value) pass through directly since
/// they are JSON-safe.
///
/// Implements `Blackboard` trait so it can be used wherever `&mut dyn Blackboard`
/// is expected.
use nexus_model::{Blackboard, BlackboardError, BoardState, Fact, FihHash, Hint, Intent};

pub struct MockGateway<B: Blackboard> {
    inner: B,
}

impl<B: Blackboard> MockGateway<B> {
    pub fn new(inner: B) -> Self {
        Self { inner }
    }
}

impl<B: Blackboard> Blackboard for MockGateway<B> {
    fn project_id(&self) -> &str {
        self.inner.project_id()
    }

    fn submit_fact(&mut self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        let decoded: Fact = serde_json::from_slice(&serde_json::to_vec(fact).unwrap()).unwrap();
        self.inner.submit_fact(&decoded)
    }

    fn submit_intent(&mut self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        let decoded: Intent = serde_json::from_slice(&serde_json::to_vec(intent).unwrap()).unwrap();
        self.inner.submit_intent(&decoded)
    }

    fn submit_hint(&mut self, hint: &Hint) -> Result<(), BlackboardError> {
        let decoded: Hint = serde_json::from_slice(&serde_json::to_vec(hint).unwrap()).unwrap();
        self.inner.submit_hint(&decoded)
    }

    fn claim_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.inner.claim_intent(intent_id, agent)
    }

    fn heartbeat(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.inner.heartbeat(intent_id, agent)
    }

    fn release_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.inner.release_intent(intent_id, agent)
    }

    fn conclude_intent(&mut self, intent_id: &str, result: &str) -> Result<Fact, BlackboardError> {
        self.inner.conclude_intent(intent_id, result)
    }

    fn read_state(&self) -> BoardState {
        let state = self.inner.read_state();
        serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap()
    }
}
