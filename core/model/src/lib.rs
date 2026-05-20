// nexus-model — Blackboard trait, capability-based Storage traits, and FIH primitives.
//
// Pure interfaces, no storage backend. Both nexus-graph and nexus-storage-sqlite
// depend on this crate only.
//
// # Capability-based trait design
//
// Instead of a monolithic Storage trait, functionality is split into fine-grained
// capability traits. Each backend implements only what it provides.
//
//   StorageRead                  — core: project_id, read_state
//    ├── FactCapable             — submit_fact
//    ├── IntentCapable           — submit/claim/heartbeat/release/conclude
//    ├── HintCapable             — submit_hint
//    ├── FilterCapable           — read_state_filtered (partial reads)
//    ├── ScanCapable             — scan_partition (bulk reads)
//    ├── EvictCapable            — approximate_size + evict_before
//    ├── FlushCapable            — flush_since (incremental export)
//    └── TimeRangeCapable        — time_range (hot/cold routing)
//
//   Aggregate: FihPersistence = FactCapable + IntentCapable + HintCapable
//   Aggregate: HotStorage     = FihPersistence + EvictCapable
//   Aggregate: ColdStorage    = FihPersistence + FilterCapable

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::ops::Range;

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

// ── Filter / Cursor types ────────────────────────────────────────────────

/// Filter for partial reads. All fields are optional; omitted fields
/// mean "no filtering on this dimension".
#[derive(Debug, Clone, Default)]
pub struct StateFilter {
    pub fact_ids: Option<Vec<String>>,
    pub intent_ids: Option<Vec<String>>,
    pub hint_ids: Option<Vec<String>>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Cursor for incremental flush. Tracks the last flushed position.
#[derive(Debug, Clone)]
pub struct FlushCursor {
    pub last_flushed_at: String,
    pub partition: String,
}

/// Result of a flush operation.
#[derive(Debug, Clone)]
pub struct FlushResult {
    pub records_flushed: u64,
    pub new_cursor: FlushCursor,
}

/// Data returned from a partition scan.
#[derive(Debug, Clone)]
pub struct PartitionData {
    pub partition: String,
    pub facts: Vec<Fact>,
    pub intents: Vec<Intent>,
    pub hints: Vec<Hint>,
}

// ── Blackboard trait — FIH lifecycle (public, stable) ─────────────────────

pub trait Blackboard {
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

// ══════════════════════════════════════════════════════════════════════════
// Capability-based Storage traits
// ══════════════════════════════════════════════════════════════════════════

// ── Core: StorageRead ────────────────────────────────────────────────────

/// Core storage trait. Every backend must implement at least this.
pub trait StorageRead: Send + Sync {
    fn project_id(&self) -> &str;
    fn read_state(&self) -> BoardState;
}

// ── Write capabilities ───────────────────────────────────────────────────

/// Backend can accept Facts.
pub trait FactCapable: StorageRead {
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError>;
}

/// Backend can manage Intents (full lifecycle).
pub trait IntentCapable: StorageRead {
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError>;
    fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError>;
    fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError>;
    fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError>;
    fn conclude_intent(
        &self,
        intent_id: &str,
        result: &serde_json::Value,
    ) -> Result<Fact, BlackboardError>;
}

/// Backend can accept Hints.
pub trait HintCapable: StorageRead {
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError>;
}

// ── Query capabilities ───────────────────────────────────────────────────

/// Backend supports filtered/partial reads.
pub trait FilterCapable: StorageRead {
    fn read_state_filtered(&self, filter: &StateFilter) -> BoardState;
}

/// Backend supports partition-based bulk scanning (for large-scale analysis).
pub trait ScanCapable: StorageRead {
    fn scan_partition(&self, partition: &str) -> Result<PartitionData, String>;
}

// ── Lifecycle capabilities ───────────────────────────────────────────────

/// Backend supports memory management / eviction (hot layer).
pub trait EvictCapable: StorageRead {
    fn approximate_size(&self) -> usize;
    fn evict_before(&self, before: &str) -> Result<u64, String>;
}

/// Backend supports incremental export (flush).
pub trait FlushCapable: StorageRead {
    fn flush_since(&self, cursor: &FlushCursor) -> Result<FlushResult, String>;
}

/// Backend reports its time coverage (for hot/cold routing).
pub trait TimeRangeCapable: StorageRead {
    fn time_range(&self) -> Option<Range<String>>;
}

// ══════════════════════════════════════════════════════════════════════════
// Aggregate traits for common backend combinations
// ══════════════════════════════════════════════════════════════════════════

/// Full FIH persistence: what a Blackboard backend must provide.
pub trait FihPersistence: FactCapable + IntentCapable + HintCapable {}
impl<T: FactCapable + IntentCapable + HintCapable> FihPersistence for T {}

/// Hot storage: full FIH + memory management (petgraph).
pub trait HotStorage: FihPersistence + EvictCapable {}
impl<T: FihPersistence + EvictCapable> HotStorage for T {}

/// Cold storage: full FIH + filtered reads (SQLite, Parquet).
pub trait ColdStorage: FihPersistence + FilterCapable {}
impl<T: FihPersistence + FilterCapable> ColdStorage for T {}

// ══════════════════════════════════════════════════════════════════════════
// DualStorage — Hot + Cold composition
// ══════════════════════════════════════════════════════════════════════════

/// Composes a Hot + Cold storage pair.
///
/// - Writes go to both hot and cold (dual-write for durability).
/// - Reads go to hot (early return, edge computing fast path).
/// - Flush/evict delegate to the appropriate layer.
pub struct DualStorage {
    hot: Box<dyn HotStorage>,
    cold: Box<dyn ColdStorage>,
}

impl DualStorage {
    pub fn new(hot: Box<dyn HotStorage>, cold: Box<dyn ColdStorage>) -> Self {
        Self { hot, cold }
    }

    pub fn hot(&self) -> &dyn HotStorage {
        &*self.hot
    }

    pub fn cold(&self) -> &dyn ColdStorage {
        &*self.cold
    }
}

// ── Core read ──

impl StorageRead for DualStorage {
    fn project_id(&self) -> &str {
        self.hot.project_id()
    }

    fn read_state(&self) -> BoardState {
        self.hot.read_state()
    }
}

// ── FIH writes: delegate to both hot + cold ──

impl FactCapable for DualStorage {
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        let hash = self.hot.submit_fact(fact)?;
        self.cold.submit_fact(fact)?;
        Ok(hash)
    }
}

impl IntentCapable for DualStorage {
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
        self.cold.conclude_intent(intent_id, result)?;
        Ok(fact)
    }
}

impl HintCapable for DualStorage {
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        self.hot.submit_hint(hint)?;
        self.cold.submit_hint(hint)?;
        Ok(())
    }
}

// ── Filtered reads: delegate to cold (hot typically doesn't support filtering) ──

impl FilterCapable for DualStorage {
    fn read_state_filtered(&self, filter: &StateFilter) -> BoardState {
        self.cold.read_state_filtered(filter)
    }
}

// ══════════════════════════════════════════════════════════════════════════
// Legacy event-log types
// ══════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEvent {
    pub event_type: String,
    pub payload: String,
}

// ══════════════════════════════════════════════════════════════════════════
// NullStorage — no-op implementation of all capability traits
// ══════════════════════════════════════════════════════════════════════════

pub struct NullStorage;

impl NullStorage {
    fn default_project_id() -> &'static str {
        "default"
    }
}

impl StorageRead for NullStorage {
    fn project_id(&self) -> &str {
        Self::default_project_id()
    }

    fn read_state(&self) -> BoardState {
        BoardState {
            facts: Vec::new(),
            intents: Vec::new(),
            hints: Vec::new(),
        }
    }
}

impl FactCapable for NullStorage {
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        Ok(fact.id.clone())
    }
}

impl IntentCapable for NullStorage {
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        Ok(intent.id.clone())
    }
    fn claim_intent(&self, _id: &str, _agent: &str) -> Result<(), BlackboardError> {
        Ok(())
    }
    fn heartbeat(&self, _id: &str, _agent: &str) -> Result<(), BlackboardError> {
        Ok(())
    }
    fn release_intent(&self, _id: &str, _agent: &str) -> Result<(), BlackboardError> {
        Ok(())
    }
    fn conclude_intent(
        &self,
        _id: &str,
        _result: &serde_json::Value,
    ) -> Result<Fact, BlackboardError> {
        Ok(Fact {
            id: FihHash("null".into()),
            origin: String::new(),
            content: serde_json::Value::Null,
            creator: String::new(),
        })
    }
}

impl HintCapable for NullStorage {
    fn submit_hint(&self, _hint: &Hint) -> Result<(), BlackboardError> {
        Ok(())
    }
}

impl FilterCapable for NullStorage {
    fn read_state_filtered(&self, _filter: &StateFilter) -> BoardState {
        BoardState {
            facts: Vec::new(),
            intents: Vec::new(),
            hints: Vec::new(),
        }
    }
}

impl ScanCapable for NullStorage {
    fn scan_partition(&self, _partition: &str) -> Result<PartitionData, String> {
        Ok(PartitionData {
            partition: _partition.to_string(),
            facts: Vec::new(),
            intents: Vec::new(),
            hints: Vec::new(),
        })
    }
}

impl EvictCapable for NullStorage {
    fn approximate_size(&self) -> usize {
        0
    }
    fn evict_before(&self, _before: &str) -> Result<u64, String> {
        Ok(0)
    }
}

impl FlushCapable for NullStorage {
    fn flush_since(&self, cursor: &FlushCursor) -> Result<FlushResult, String> {
        Ok(FlushResult {
            records_flushed: 0,
            new_cursor: cursor.clone(),
        })
    }
}

impl TimeRangeCapable for NullStorage {
    fn time_range(&self) -> Option<Range<String>> {
        None
    }
}
