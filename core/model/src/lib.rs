// nexus-model — Blackboard trait, Storage trait, and FIH primitives.
//
// Pure interfaces, no storage backend. Both nexus-graph and nexus-table
// depend on this crate only.

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

// ── Blackboard trait — FIH lifecycle (public, stable) ─────────────────────

pub trait Blackboard {
    /// Returns the project scope this blackboard is operating on.
    /// Implementations that don't support multi-project return "default".
    fn project_id(&self) -> &str {
        "default"
    }

    fn submit_fact(&mut self, fact: &Fact) -> Result<FihHash, BlackboardError>;
    fn submit_hint(&mut self, hint: &Hint) -> Result<(), BlackboardError>;
    fn submit_intent(&mut self, intent: &Intent) -> Result<FihHash, BlackboardError>;
    fn claim_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError>;
    fn heartbeat(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError>;
    fn release_intent(&mut self, intent_id: &str, agent: &str) -> Result<(), BlackboardError>;
    fn conclude_intent(
        &mut self,
        intent_id: &str,
        result: &serde_json::Value,
    ) -> Result<Fact, BlackboardError>;
    fn read_state(&self) -> BoardState;
}

// Blanket impl: &mut T delegates to T for any Blackboard implementor.
impl<T: Blackboard> Blackboard for &mut T {
    fn project_id(&self) -> &str {
        (**self).project_id()
    }
    fn submit_fact(&mut self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        (**self).submit_fact(fact)
    }
    fn submit_hint(&mut self, hint: &Hint) -> Result<(), BlackboardError> {
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
    ) -> Result<Fact, BlackboardError> {
        (**self).conclude_intent(intent_id, result)
    }
    fn read_state(&self) -> BoardState {
        (**self).read_state()
    }
}

// ── Unified Storage trait — FIH CRUD, thread-safe via &self ──────────────

/// Unified storage interface for FIH persistence.
/// Thread-safe via &self (internal mutability, e.g. Mutex<Connection>).
#[allow(missing_docs)]
pub trait Storage: Send + Sync {
    fn project_id(&self) -> &str;
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError>;
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError>;
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError>;
    fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError>;
    fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError>;
    fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError>;
    fn conclude_intent(
        &self,
        intent_id: &str,
        result: &serde_json::Value,
    ) -> Result<Fact, BlackboardError>;
    fn read_state(&self) -> BoardState;
}

// ── HotStorage marker — in-memory fast path ──────────────────────────────

/// Hot storage: in-memory, fast, early return for edge computing.
#[allow(missing_docs)]
pub trait HotStorage: Storage {}

// ── ColdStorage marker — durable backend ─────────────────────────────────

/// Cold storage: durable backend (SQLite, Parquet, etc.).
#[allow(missing_docs)]
pub trait ColdStorage: Storage {}

// ── DualStorage — composes Hot + Cold ────────────────────────────────────

/// Composes a Hot + Cold storage pair.
/// - Reads go to hot (early return, edge computing fast path)
/// - Writes go to hot first, then cold (dual-write for durability)
/// - Flush: periodic sync from hot to cold
pub struct DualStorage {
    hot: Box<dyn HotStorage>,
    cold: Box<dyn ColdStorage>,
}

#[allow(missing_docs)]
impl DualStorage {
    pub fn new(hot: Box<dyn HotStorage>, cold: Box<dyn ColdStorage>) -> Self {
        Self { hot, cold }
    }

    /// Flush hot state to cold storage.
    /// Called periodically to ensure durability.
    ///
    /// For the initial implementation where every write is dual-written
    /// (hot + cold on every mutation), flush is a no-op. Future
    /// async/batched cold write strategies would use this method
    /// to batch-persist pending operations.
    pub fn flush(&self) -> Result<(), String> {
        // no-op: every write is already dual-written
        Ok(())
    }
}

impl Storage for DualStorage {
    fn project_id(&self) -> &str {
        // Both hot and cold should agree on project_id; use hot for speed.
        self.hot.project_id()
    }

    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        let hash = self.hot.submit_fact(fact)?;
        // Propagate to cold for durability.
        self.cold.submit_fact(fact)?;
        Ok(hash)
    }

    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        self.hot.submit_hint(hint)?;
        self.cold.submit_hint(hint)?;
        Ok(())
    }

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
        // For conclusion, the result is a new Fact; write it to cold too.
        self.cold.conclude_intent(intent_id, result)?;
        Ok(fact)
    }

    fn read_state(&self) -> BoardState {
        // Reads go to hot for the fast path (edge computing).
        self.hot.read_state()
    }
}

// ── Legacy event-log types ────────────────────────────────────────────────

/// A single stored event in a legacy event-log backend.
/// Used by SqliteStorage and GraphBlackboard for persistence replay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEvent {
    pub event_type: String,
    pub payload: String,
}

// ── Null storage (no persistence) ─────────────────────────────────────────

/// No-op storage. All FIH operations are discarded.
/// Implements Storage, HotStorage, and ColdStorage for testing and
/// default construction.
pub struct NullStorage;

impl NullStorage {
    fn default_project_id() -> &'static str {
        "default"
    }
}

#[allow(missing_docs)]
impl Storage for NullStorage {
    fn project_id(&self) -> &str {
        Self::default_project_id()
    }

    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        Ok(fact.id.clone())
    }

    fn submit_hint(&self, _hint: &Hint) -> Result<(), BlackboardError> {
        Ok(())
    }

    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        Ok(intent.id.clone())
    }

    fn claim_intent(&self, _intent_id: &str, _agent: &str) -> Result<(), BlackboardError> {
        Ok(())
    }

    fn heartbeat(&self, _intent_id: &str, _agent: &str) -> Result<(), BlackboardError> {
        Ok(())
    }

    fn release_intent(&self, _intent_id: &str, _agent: &str) -> Result<(), BlackboardError> {
        Ok(())
    }

    fn conclude_intent(
        &self,
        _intent_id: &str,
        _result: &serde_json::Value,
    ) -> Result<Fact, BlackboardError> {
        Ok(Fact {
            id: FihHash("null".into()),
            origin: String::new(),
            content: serde_json::Value::Null,
            creator: String::new(),
        })
    }

    fn read_state(&self) -> BoardState {
        BoardState {
            facts: Vec::new(),
            intents: Vec::new(),
            hints: Vec::new(),
        }
    }
}

impl HotStorage for NullStorage {}

impl ColdStorage for NullStorage {}
