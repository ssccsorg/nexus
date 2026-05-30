use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ── Content-addressable identifier ───────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    pub to_fact_id: Option<String>,
    pub description: String,
    pub creator: String,
    pub worker: Option<String>,
    pub last_heartbeat_at: Option<String>,
    pub created_at: Option<String>,
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
