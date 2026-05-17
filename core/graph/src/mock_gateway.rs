/// Mock gateway for transport-layer testing.
///
/// A transparent proxy that round-trips every FIH primitive through JSON
/// serialization before reaching the inner Blackboard. This simulates the
/// exact serialization boundary that a real HTTP gateway (axum in
/// gateway/api/) would impose.
///
/// Any scenario test that passes through `MockGateway` validates that the
/// FIH protocol works correctly across a JSON transport boundary — the same
/// boundary that separates external agents from the Blackboard.
use crate::{Blackboard, BlackboardError, BoardState, Fact, FihHash, Hint, Intent};

pub struct MockGateway<B: Blackboard> {
    inner: B,
}

impl<B: Blackboard> MockGateway<B> {
    pub fn new(inner: B) -> Self {
        Self { inner }
    }

    pub fn submit_fact(&mut self, fact: &Fact) -> FihHash {
        let decoded: Fact = serde_json::from_slice(&serde_json::to_vec(fact).unwrap()).unwrap();
        self.inner.submit_fact(&decoded)
    }

    pub fn submit_intent(&mut self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        let decoded: Intent =
            serde_json::from_slice(&serde_json::to_vec(intent).unwrap()).unwrap();
        self.inner.submit_intent(&decoded)
    }

    pub fn submit_hint(&mut self, hint: &Hint) {
        let decoded: Hint =
            serde_json::from_slice(&serde_json::to_vec(hint).unwrap()).unwrap();
        self.inner.submit_hint(&decoded)
    }

    pub fn claim_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let id: String =
            serde_json::from_slice(&serde_json::to_vec(intent_id).unwrap()).unwrap();
        let a: String = serde_json::from_slice(&serde_json::to_vec(agent).unwrap()).unwrap();
        self.inner.claim_intent(&id, &a)
    }

    pub fn heartbeat(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let id: String =
            serde_json::from_slice(&serde_json::to_vec(intent_id).unwrap()).unwrap();
        let a: String = serde_json::from_slice(&serde_json::to_vec(agent).unwrap()).unwrap();
        self.inner.heartbeat(&id, &a)
    }

    pub fn release_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let id: String =
            serde_json::from_slice(&serde_json::to_vec(intent_id).unwrap()).unwrap();
        let a: String = serde_json::from_slice(&serde_json::to_vec(agent).unwrap()).unwrap();
        self.inner.release_intent(&id, &a)
    }

    pub fn conclude_intent(
        &mut self,
        intent_id: &str,
        result: &serde_json::Value,
    ) -> Result<(Fact, Vec<Intent>), BlackboardError> {
        let id: String =
            serde_json::from_slice(&serde_json::to_vec(intent_id).unwrap()).unwrap();
        let r: serde_json::Value =
            serde_json::from_slice(&serde_json::to_vec(result).unwrap()).unwrap();
        self.inner.conclude_intent(&id, &r)
    }

    pub fn read_state(&self) -> BoardState {
        let state = self.inner.read_state();
        serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GraphBlackboard;

    #[test]
    fn test_mock_gateway_submit_fact() {
        let mut gw = MockGateway::new(GraphBlackboard::new());
        let fact = Fact {
            id: FihHash("f_mock_001".into()),
            origin: "mock-test".into(),
            content: serde_json::Value::String("Mock gateway test".into()),
            creator: "tester".into(),
        };
        let hash = gw.submit_fact(&fact);
        assert_eq!(hash.0, "f_mock_001");

        let state = gw.read_state();
        assert_eq!(state.facts.len(), 1);
        assert_eq!(state.facts[0].content, "Mock gateway test");
    }
}
