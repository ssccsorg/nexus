/// Mock gateway for transport-layer testing.
///
/// A transparent proxy that round-trips structured FIH types (Fact, Intent,
/// Hint, BoardState) through JSON serialisation before reaching the inner
/// Blackboard. Primitive types (strings, Value) pass through directly since
/// they are JSON-safe.
///
/// Implements `Blackboard` trait so it can be used wherever `&mut dyn Blackboard`
/// is expected.
use nexus_model::{Blackboard, BlackboardError, BoardState, Fact, FactCapable, FihHash, Hint, HintCapable, Intent, IntentCapable, StorageRead};

pub struct MockGateway<B: Blackboard> {
    inner: B,
}

impl<B: Blackboard> MockGateway<B> {
    pub fn new(inner: B) -> Self {
        Self { inner }
    }
}

// ── StorageRead — delegates to inner ─────────────────────────────────────

impl<B: Blackboard> StorageRead for MockGateway<B> {
    fn project_id(&self) -> &str {
        self.inner.project_id()
    }

    fn read_state(&self) -> BoardState {
        let state = self.inner.read_state();
        serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap()
    }
}

// ── FactCapable — JSON round-trip before delegate ───────────────────────

impl<B: Blackboard> FactCapable for MockGateway<B> {
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        let decoded: Fact = serde_json::from_slice(&serde_json::to_vec(fact).unwrap()).unwrap();
        self.inner.submit_fact(&decoded)
    }
}

// ── HintCapable — JSON round-trip before delegate ───────────────────────

impl<B: Blackboard> HintCapable for MockGateway<B> {
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        let decoded: Hint = serde_json::from_slice(&serde_json::to_vec(hint).unwrap()).unwrap();
        self.inner.submit_hint(&decoded)
    }
}

// ── IntentCapable — JSON round-trip for submit, pass-through otherwise ──

impl<B: Blackboard> IntentCapable for MockGateway<B> {
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        let decoded: Intent = serde_json::from_slice(&serde_json::to_vec(intent).unwrap()).unwrap();
        self.inner.submit_intent(&decoded)
    }

    fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.inner.claim_intent(intent_id, agent)
    }

    fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.inner.heartbeat(intent_id, agent)
    }

    fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.inner.release_intent(intent_id, agent)
    }

    fn conclude_intent(&self, intent_id: &str, result: &str) -> Result<Fact, BlackboardError> {
        self.inner.conclude_intent(intent_id, result)
    }
}
