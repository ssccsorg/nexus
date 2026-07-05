// ── Contract Layer: Governance gate + Evidence chain + Hint engine ─────
//
// The contract layer is a governance wrapper over FihStorage, following the
// same structural pattern as FihBlackboard (storage/fih.rs):
//
//   FihBlackboard<I> { storage: FihStorage<I> }   ← sync wrapper (block_on)
//   ContractBlackboard<I> { storage: FihStorage<I> } ← governance wrapper
//
// ContractBlackboard wraps FihStorage and intercepts write operations to
// apply governance gates (schema admission, hint evaluation) and record
// evidence (SHA-256 chain). Read operations pass through directly.
//
// The three governance primitives:
//   GovernanceGate  — schema-based write admission (raw data → schemed-segment)
//   HintEngine      — constraint evaluation during Intent resolution
//   EvidenceChain   — append-only SHA-256 chain for tamper evidence
//
// When `enabled` is false, the contract passes through to storage with zero
// governance overhead. This preserves full backward compatibility.
//
// wasm32-unknown-unknown: All types in this module compile under wasm
// because they use only std primitives + sha2 (pure Rust, no host fns).

pub mod evidence;
pub mod gate;
pub mod hint;
pub mod lifecycle;

use std::sync::Arc;
use std::time::SystemTime;

pub use evidence::{EvidenceChain, EvidenceEntry};
pub use gate::{GovernanceBypassError, GovernanceGate};
pub use hint::{HintEngine, HintRule};
pub use lifecycle::{HealthStatus, NexConfig, NexInstanceInfo, NexLifecycle};

use nexus_model::error::BlackboardError;
use nexus_model::fih::{Fact, FihHash, Intent};
use nexus_model::{AsyncFactCapable, AsyncIntentCapable, AsyncStorageRead};

use crate::io::FileIo;
use crate::storage::core::FihStorage;

// ── ContractBlackboard ─────────────────────────────────────────────────

/// Governance wrapper over FihStorage.
///
/// Follows the same structural pattern as `FihBlackboard` (storage/fih.rs):
/// wraps FihStorage in a lightweight struct that adds a cross-cutting concern
/// (governance vs. sync interface) while delegating the core storage logic.
///
/// Governance flow:
///   submit_fact(fact)
///     1. gate.admit(fact.origin, fact.content.data)    ← schema check
///     2. storage.submit_fact(fact)                      ← pass-through
///     3. evidence.append(fact.id, "fact:submit")        ← audit
///
///   conclude_intent(id, result)
///     1. hints.check_numeric(result)                    ← constraint gate
///     2. storage.conclude_intent(id, result)            ← pass-through
///     3. evidence.append(id, "intent:conclude")         ← audit
pub struct ContractBlackboard<I: FileIo> {
    /// The underlying FIH storage instance.
    pub storage: Arc<FihStorage<I>>,
    /// Schema-based write admission gate.
    pub gate: GovernanceGate,
    /// Constraint evaluation engine.
    pub hints: HintEngine,
    /// Append-only SHA-256 audit chain.
    pub evidence: EvidenceChain,
    /// Whether governance checks are active.
    enabled: bool,
}

impl<I: FileIo> ContractBlackboard<I> {
    /// Create a new contract blackboard wrapping the given storage.
    ///
    /// When `enabled` is true, governance gates are applied to write
    /// operations. When false, all operations pass through to storage
    /// with zero governance overhead.
    pub fn new(storage: Arc<FihStorage<I>>, enabled: bool) -> Self {
        Self {
            storage,
            gate: GovernanceGate::new(),
            hints: HintEngine::new(),
            evidence: EvidenceChain::new(),
            enabled,
        }
    }

    /// Create a contract blackboard with governance enabled.
    pub fn enabled(storage: Arc<FihStorage<I>>) -> Self {
        Self::new(storage, true)
    }

    /// Create a contract blackboard with governance disabled (pass-through).
    pub fn disabled(storage: Arc<FihStorage<I>>) -> Self {
        Self::new(storage, false)
    }

    /// Returns true if governance is active.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enable or disable governance at runtime.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Register a schema via the governance gate.
    /// Returns the SHA-256 hex hash of the schema content.
    pub fn register_schema(&self, schema_id: &str, schema: &[u8]) -> String {
        self.gate.register_schema(schema_id, schema)
    }

    /// Check numeric constraints against all active hints.
    pub fn check_hints(&self, value: i64) -> Result<(), String> {
        self.hints.check_numeric(value)
    }

    /// Return the evidence chain tip hash.
    pub fn evidence_tip(&self) -> Option<String> {
        self.evidence.tip()
    }

    /// Verify evidence chain integrity from sequence number.
    pub fn verify_evidence(&self, from_seq: u64) -> bool {
        self.evidence.verify(from_seq)
    }

    /// Record a custom action in the evidence chain.
    pub fn record_evidence(&self, action_hash: &str, action_type: &str) {
        let ts = nanos();
        self.evidence.append(action_hash, action_type, ts);
    }

    /// Return the project ID from the underlying storage.
    pub fn project_id(&self) -> &str {
        self.storage.project_id()
    }
}

// ── AsyncStorageRead: pass-through ─────────────────────────────────────

impl<I: FileIo> AsyncStorageRead for ContractBlackboard<I> {
    fn project_id(&self) -> &str {
        self.storage.project_id()
    }

    async fn read_state(&self) -> nexus_model::BoardState {
        self.storage.read_state().await
    }
}

// ── AsyncFactCapable: governed submit ──────────────────────────────────

impl<I: FileIo> AsyncFactCapable for ContractBlackboard<I> {
    async fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        if self.enabled {
            // Step 1: Governance gate — schema admission
            self.gate
                .admit(&fact.origin, &fact.content.data)
                .map_err(|e| BlackboardError::Forbidden(e.to_string()))?;

            // Step 2: Storage write
            let hash = self.storage.submit_fact(fact).await?;

            // Step 3: Evidence chain
            let ts = nanos();
            self.evidence.append(&hash.to_string(), "fact:submit", ts);

            Ok(hash)
        } else {
            self.storage.submit_fact(fact).await
        }
    }
}

// ── AsyncIntentCapable: governed submit / conclude ─────────────────────

impl<I: FileIo> AsyncIntentCapable for ContractBlackboard<I> {
    async fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        if self.enabled {
            let hash = self.storage.submit_intent(intent).await?;
            let ts = nanos();
            self.evidence.append(&hash.to_string(), "intent:submit", ts);
            Ok(hash)
        } else {
            self.storage.submit_intent(intent).await
        }
    }

    async fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.storage.claim_intent(intent_id, agent).await
    }

    async fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.storage.heartbeat(intent_id, agent).await
    }

    async fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.storage.release_intent(intent_id, agent).await
    }

    async fn conclude_intent(
        &self,
        intent_id: &str,
        result: &str,
    ) -> Result<Fact, BlackboardError> {
        if self.enabled {
            // Step 1: Hint engine — constraint evaluation
            if let Ok(numeric) = result.trim().parse::<i64>() {
                self.hints
                    .check_numeric(numeric)
                    .map_err(|e| BlackboardError::Forbidden(e))?;
            }

            // Step 2: Storage conclude
            let fact = self.storage.conclude_intent(intent_id, result).await?;

            // Step 3: Evidence chain
            let ts = nanos();
            self.evidence.append(intent_id, "intent:conclude", ts);

            Ok(fact)
        } else {
            self.storage.conclude_intent(intent_id, result).await
        }
    }
}

// ── Timestamp helper ───────────────────────────────────────────────────

fn nanos() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}
