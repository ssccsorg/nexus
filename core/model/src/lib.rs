// nexus-model — Blackboard trait and FIH primitives.
//
// Pure interface, no storage backend. Both GraphBlackboard and SqlBlackboard
// implement this trait. Modules depend only on this crate.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ── Error type ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum BlackboardError {
    NotFound(String),
    Conflict(String),
    Forbidden(String),
    Internal(String),
}

impl std::fmt::Display for BlackboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(m) => write!(f, "not found: {m}"),
            Self::Conflict(m) => write!(f, "conflict: {m}"),
            Self::Forbidden(m) => write!(f, "forbidden: {m}"),
            Self::Internal(m) => write!(f, "internal: {m}"),
        }
    }
}

impl std::error::Error for BlackboardError {}

// ── Content-addressable identifier ───────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FihHash(pub String);

impl std::fmt::Display for FihHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FihHash {
    pub fn new(fields: &[&str], type_tag: &str) -> Self {
        let mut h = Sha256::new();
        for f in fields {
            h.update(f.as_bytes());
        }
        h.update(type_tag.as_bytes());
        Self(format!("{:x}", h.finalize()))
    }

    pub fn chain(a: &FihHash, b: &FihHash, c: &FihHash) -> FihHash {
        let mut h = Sha256::new();
        h.update(a.0.as_bytes());
        h.update(b.0.as_bytes());
        h.update(c.0.as_bytes());
        Self(format!("{:x}", h.finalize()))
    }
}

// ── FIH Primitives ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    pub id: FihHash,
    pub origin: String,
    pub content: serde_json::Value,
    pub creator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub id: FihHash,
    pub from_facts: Vec<String>,
    pub description: String,
    pub creator: String,
    pub worker: Option<String>,
    pub concluded_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hint {
    pub id: FihHash,
    pub content: String,
    pub creator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardState {
    pub facts: Vec<Fact>,
    pub intents: Vec<Intent>,
    pub hints: Vec<Hint>,
}

// ── Blackboard trait — FIH lifecycle (public, stable) ─────────────────────

pub trait Blackboard {
    fn submit_fact(&mut self, fact: &Fact) -> FihHash;
    fn submit_hint(&mut self, hint: &Hint);
    fn submit_intent(&mut self, intent: &Intent) -> Result<FihHash, BlackboardError>;
    fn claim_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError>;
    fn heartbeat(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError>;
    fn release_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError>;
    fn conclude_intent(
        &mut self,
        intent_id: &str,
        result: &serde_json::Value,
    ) -> Result<(Fact, Vec<Intent>), BlackboardError>;
    fn read_state(&self) -> BoardState;
}

// Blanket impl: &mut T delegates to T for any Blackboard implementor.
// Enables shared access through references (e.g., MockGateway with &mut GraphBlackboard).
impl<T: Blackboard> Blackboard for &mut T {
    fn submit_fact(&mut self, fact: &Fact) -> FihHash {
        (**self).submit_fact(fact)
    }
    fn submit_hint(&mut self, hint: &Hint) {
        (**self).submit_hint(hint)
    }
    fn submit_intent(&mut self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        (**self).submit_intent(intent)
    }
    fn claim_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        (**self).claim_intent(intent_id, agent)
    }
    fn heartbeat(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        (**self).heartbeat(intent_id, agent)
    }
    fn release_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        (**self).release_intent(intent_id, agent)
    }
    fn conclude_intent(
        &mut self,
        intent_id: &str,
        result: &serde_json::Value,
    ) -> Result<(Fact, Vec<Intent>), BlackboardError> {
        (**self).conclude_intent(intent_id, result)
    }
    fn read_state(&self) -> BoardState {
        (**self).read_state()
    }
}
