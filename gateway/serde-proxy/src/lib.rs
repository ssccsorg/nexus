/// Serialization boundary proxy for cross-boundary FIH communication.
///
/// A transparent proxy that round-trips structured FIH types (Fact, Intent,
/// Hint, BoardState) through serde serialisation before reaching the inner
/// Blackboard. This simulates the serialization boundary of an HTTP transport
/// or RPC call without requiring actual network I/O.
///
/// Currently uses JSON (serde_json). The encoding is an implementation detail
/// and may switch to bincode or other binary formats in the future.
///
/// Implements `Blackboard` trait so it can be used wherever a Blackboard
/// reference is expected.
use nexus_model::{
    Blackboard, BlackboardError, BoardState, Fact, FactCapable, FihHash, Hint, HintCapable, Intent,
    IntentCapable, StorageRead,
};

pub struct SerdeProxy<B: Blackboard> {
    inner: B,
}

impl<B: Blackboard> SerdeProxy<B> {
    pub fn new(inner: B) -> Self {
        Self { inner }
    }
}

// ── StorageRead — delegates to inner ─────────────────────────────────────

impl<B: Blackboard> StorageRead for SerdeProxy<B> {
    fn project_id(&self) -> &str {
        self.inner.project_id()
    }

    fn read_state(&self) -> BoardState {
        let state = self.inner.read_state();
        serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap()
    }
}

// ── FactCapable — serde round-trip before delegate ───────────────────────

impl<B: Blackboard> FactCapable for SerdeProxy<B> {
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        let decoded: Fact = serde_json::from_slice(&serde_json::to_vec(fact).unwrap()).unwrap();
        self.inner.submit_fact(&decoded)
    }
}

// ── HintCapable — serde round-trip before delegate ───────────────────────

impl<B: Blackboard> HintCapable for SerdeProxy<B> {
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        let decoded: Hint = serde_json::from_slice(&serde_json::to_vec(hint).unwrap()).unwrap();
        self.inner.submit_hint(&decoded)
    }
}

// ── IntentCapable — serde round-trip for submit, pass-through otherwise ──

impl<B: Blackboard> IntentCapable for SerdeProxy<B> {
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
