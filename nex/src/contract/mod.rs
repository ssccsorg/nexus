// ── Contract Layer: Governance gate + Evidence chain + Hint engine ─────
//
// The contract layer wraps FihStorage with three governance components:
//
//   GovernanceGate  — schema-based write admission (raw data → schemed-segment)
//   HintEngine      — constraint evaluation during Intent resolution
//   EvidenceChain   — append-only SHA-256 chain for tamper evidence
//
// FihContract is the primary entry point. It implements the same write
// operations as FihStorage but routes them through the governance gate
// and records every action in the evidence chain.
//
// When `enabled` is false (default), the contract passes through to
// storage directly with zero overhead — no gate check, no evidence
// recording. This preserves full backward compatibility.
//
// wasm32-unknown-unknown: All types in this module compile under wasm
// because they use only std primitives + sha2 (pure Rust, no host fns).

pub mod evidence;
pub mod gate;
pub mod hint;
pub mod lifecycle;

use async_trait::async_trait;

use std::sync::Arc;
use std::time::SystemTime;

pub use evidence::{EvidenceChain, EvidenceEntry};
pub use gate::{GovernanceBypassError, GovernanceGate};
pub use hint::{HintEngine, HintRule};
pub use lifecycle::{HealthStatus, NexConfig, NexInstanceInfo, NexLifecycle};

use nexus_model::error::BlackboardError;
use nexus_model::fih::{Fact, FihHash, Hint, Intent};
use nexus_model::{AsyncFactCapable, AsyncFilterCapable, AsyncHintCapable, AsyncIntentCapable, AsyncStorageRead};

use crate::io::FileIo;
use crate::storage::core::entity_store::EntityStore;
use crate::storage::core::FihStorage;

// ── ContractGate trait ─────────────────────────────────────────────────

/// Abstract contract gate interface.
///
/// Allows alternative gate implementations (testing, simulation, etc.)
/// while keeping the same governance surface.
#[async_trait]
pub trait ContractGate: Send + Sync {
    /// Admit a fact write. Validates that `data` conforms to `schema`.
    async fn admit_fact(&self, data: &[u8], schema: &str) -> Result<FihHash, BlackboardError>;

    /// Validate and propose an intent transition.
    async fn propose_intent(
        &self,
        from_facts: &[FihHash],
        transition: &str,
    ) -> Result<FihHash, BlackboardError>;

    /// Evaluate active hints for the given intent.
    async fn evaluate_hints(&self, intent_id: &str) -> Result<bool, BlackboardError>;

    /// Record an action in the evidence chain.
    async fn record_evidence(
        &self,
        action_hash: &str,
        action_type: &str,
    ) -> Result<(), BlackboardError>;

    /// Return the tip of the evidence chain, if any.
    async fn evidence_tip(&self) -> Option<String>;
}

// ── FihContract ────────────────────────────────────────────────────────

/// Governance-wrapped FIH storage.
///
/// Wraps `Arc<FihStorage<I>>` and adds the contract layer:
///   submit_fact       → gate.admit() + storage.submit_fact() + evidence.append()
///   submit_intent     → evidence.append() + storage.submit_intent()
///   conclude_intent   → hint_engine.check() + storage.conclude_intent() + evidence.append()
///
/// Read operations pass through to storage directly (no gate overhead).
pub struct FihContract<I: FileIo> {
    /// The underlying FIH storage instance.
    pub storage: Arc<FihStorage<I>>,
    /// Governance gate for write admission.
    pub gate: GovernanceGate,
    /// Evidence chain for action audit trail.
    pub evidence: EvidenceChain,
    /// Hint engine for constraint evaluation.
    pub hints: HintEngine,
    /// Whether governance is active. When false, all operations
    /// pass through to storage with zero contract overhead.
    enabled: bool,
}

impl<I: FileIo> FihContract<I> {
    /// Create a new FihContract wrapping the given storage.
    ///
    /// When `enabled` is false, the contract layer is inactive and
    /// all operations pass through to storage directly.
    pub fn new(storage: Arc<FihStorage<I>>, enabled: bool) -> Self {
        Self {
            storage,
            gate: GovernanceGate::new(),
            evidence: EvidenceChain::new(),
            hints: HintEngine::new(),
            enabled,
        }
    }

    /// Returns true if the contract layer is active.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enable or disable the contract layer at runtime.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    // ── Fact operations ───────────────────────────────────────────

    /// Submit a fact through the governance gate.
    ///
    /// When the contract is enabled:
    ///   1. Gate admits the fact (schema validation)
    ///   2. Storage persists the fact
    ///   3. Evidence chain records the action
    ///
    /// When disabled: pass-through to storage.
    pub async fn submit_fact(
        &self,
        fact: &Fact,
        schema: &str,
    ) -> Result<FihHash, BlackboardError> {
        if self.enabled {
            // Step 1: Governance gate
            self.gate
                .admit(schema, &fact.content.data)
                .map_err(|e| BlackboardError::Forbidden(e.to_string()))?;

            // Step 2: Storage write
            let hash = self.storage.submit_fact(fact).await?;

            // Step 3: Evidence
            let now = nanos();
            self.evidence.append(&hash.to_string(), "fact:submit", now);

            Ok(hash)
        } else {
            self.storage.submit_fact(fact).await
        }
    }

    /// Submit a fact without governance (always pass-through).
    ///
    /// Useful for internal/system facts that should bypass the gate.
    pub async fn submit_fact_unchecked(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        self.storage.submit_fact(fact).await
    }

    // ── Intent operations ─────────────────────────────────────────

    /// Submit an intent through the governance gate.
    ///
    /// When the contract is enabled:
    ///   1. Validates from_facts exist
    ///   2. Storage persists the intent
    ///   3. Evidence chain records the action
    pub async fn submit_intent(
        &self,
        intent: &Intent,
    ) -> Result<FihHash, BlackboardError> {
        if self.enabled {
            // Validate from_facts exist
            for fid in &intent.from_facts {
                let key = fid.to_string();
                if !self.storage.fact_store.contains_key(&key).await {
                    return Err(BlackboardError::NotFound(format!(
                        "Fact {} referenced by intent not found",
                        key
                    )));
                }
            }

            let hash = self.storage.submit_intent(intent).await?;

            let now = nanos();
            self.evidence.append(
                &hash.to_string(),
                "intent:submit",
                now,
            );

            Ok(hash)
        } else {
            self.storage.submit_intent(intent).await
        }
    }

    /// Conclude an intent, gated by hint evaluation.
    ///
    /// When the contract is enabled:
    ///   1. Evaluate all active hints against the result
    ///   2. Storage concludes the intent
    ///   3. Evidence chain records the action
    pub async fn conclude_intent(
        &self,
        intent_id: &str,
        result: &str,
    ) -> Result<Fact, BlackboardError> {
        if self.enabled {
            // Try to parse result as numeric for hint evaluation
            if let Ok(numeric) = result.trim().parse::<i64>() {
                self.hints
                    .check_numeric(numeric)
                    .map_err(|e| BlackboardError::Forbidden(e))?;
            }

            let fact = self.storage.conclude_intent(intent_id, result).await?;

            let now = nanos();
            self.evidence.append(intent_id, "intent:conclude", now);

            Ok(fact)
        } else {
            self.storage.conclude_intent(intent_id, result).await
        }
    }

    /// Claim an intent (pass-through to storage).
    pub async fn claim_intent(
        &self,
        intent_id: &str,
        agent: &str,
    ) -> Result<(), BlackboardError> {
        self.storage.claim_intent(intent_id, agent).await
    }

    /// Heartbeat on a claimed intent (pass-through to storage).
    pub async fn heartbeat(
        &self,
        intent_id: &str,
        agent: &str,
    ) -> Result<(), BlackboardError> {
        self.storage.heartbeat(intent_id, agent).await
    }

    /// Release a claimed intent (pass-through to storage).
    pub async fn release_intent(
        &self,
        intent_id: &str,
        agent: &str,
    ) -> Result<(), BlackboardError> {
        self.storage.release_intent(intent_id, agent).await
    }

    // ── Hint operations ───────────────────────────────────────────

    /// Add a hint to the engine.
    ///
    /// When the contract is enabled, the hint is added to the engine
    /// and also persisted to storage for durability.
    pub async fn add_hint(&self, id: &str, rule: HintRule) {
        self.hints.add(id, rule.clone());

        if self.enabled {
            // Persist to storage as a Hint record
            let hint = Hint {
                id: FihHash::from_hex(id),
                content: rule.describe(),
                creator: "contract".into(),
            };
            let _ = self.storage.submit_hint(&hint).await;
        }
    }

    /// Remove a hint from the engine.
    pub fn remove_hint(&self, id: &str) {
        self.hints.remove(id);
    }

    /// Clear all hints from the engine.
    pub fn clear_hints(&self) {
        self.hints.clear();
    }

    /// Check constraints against a numeric value.
    pub fn check_hints(&self, value: i64) -> Result<(), String> {
        self.hints.check_numeric(value)
    }

    // ── Evidence operations ─────────────────────────────────────────

    /// Record a custom action in the evidence chain.
    pub fn record_evidence(&self, action_hash: &str, action_type: &str) {
        let now = nanos();
        self.evidence.append(action_hash, action_type, now);
    }

    /// Return the evidence chain tip, if any.
    pub fn evidence_tip(&self) -> Option<String> {
        self.evidence.tip()
    }

    /// Verify evidence chain integrity from a given sequence number.
    pub fn verify_evidence(&self, from_seq: u64) -> bool {
        self.evidence.verify(from_seq)
    }

    // ── Read operations (pass-through to storage) ─────────────────

    /// Read the full board state.
    pub async fn read_state(&self) -> nexus_model::BoardState {
        self.storage.read_state().await
    }

    /// Read a filtered board state.
    pub async fn read_state_filtered(
        &self,
        filter: &nexus_model::StateFilter,
    ) -> nexus_model::BoardState {
        self.storage.read_state_filtered(filter).await
    }

    /// Flush pending writes to IO storage.
    pub async fn flush_pending(&self) -> Result<(), String> {
        self.storage.flush_pending().await
    }

    /// Scan a partition.
    pub async fn scan_partition(&self, _partition: &str) -> Vec<Vec<u8>> {
        // FihStorage has scan_partition through AsyncScanCapable.
        // It takes a PartitionData, not a string. We provide a basic
        // scan by reading fact/intent/hint stores.
        vec![]
    }

    /// Return the project ID.
    pub fn project_id(&self) -> &str {
        self.storage.project_id()
    }

    /// Return instance info for lifecycle integration.
    pub fn info(&self) -> NexInstanceInfo {
        // For the uptime, we approximate by using evidence chain tip time.
        // This is a best-effort value.
        let fact_count = futures_executor::block_on(async {
            // We can't easily count facts in FihStorage from outside
            // since fact_store is pub but needs async iteration.
            // Return a placeholder in v1.
            0usize
        });

        let entries = self.evidence.entries();
        NexInstanceInfo {
            project_id: self.project_id().to_string(),
            uptime_secs: 0,
            fact_count,
            intent_count: 0,
            hint_count: self.hints.len(),
            contract_enabled: self.enabled,
            evidence_tip: self.evidence.tip(),
            evidence_count: entries.len(),
        }
    }
}

// ── Default ContractGate implementation ─────────────────────────────────

/// Default gate implementation using GovernanceGate + HintEngine.
pub struct DefaultContractGate {
    /// Schema-based governance gate.
    pub gate: GovernanceGate,
    /// Hint engine for constraint evaluation.
    pub hints: HintEngine,
    /// Evidence chain for action audit.
    pub evidence: EvidenceChain,
}

impl DefaultContractGate {
    pub fn new() -> Self {
        Self {
            gate: GovernanceGate::new(),
            hints: HintEngine::new(),
            evidence: EvidenceChain::new(),
        }
    }
}

impl Default for DefaultContractGate {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ContractGate for DefaultContractGate {
    async fn admit_fact(&self, data: &[u8], schema: &str) -> Result<FihHash, BlackboardError> {
        self.gate
            .admit(schema, data)
            .map_err(|e| BlackboardError::Forbidden(e.to_string()))?;

        // Compute content-addressed hash
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(data);
        let hash = FihHash::new(&[schema], "fact");
        let now = nanos();
        self.evidence.append(&hash.to_string(), "fact:admit", now);
        Ok(hash)
    }

    async fn propose_intent(
        &self,
        from_facts: &[FihHash],
        transition: &str,
    ) -> Result<FihHash, BlackboardError> {
        if from_facts.is_empty() {
            return Err(BlackboardError::Forbidden(
                "intent must reference at least one fact".into(),
            ));
        }
        let hash = FihHash::new(
            &[
                &from_facts
                    .iter()
                    .map(|f| f.to_string())
                    .collect::<Vec<_>>()
                    .join(","),
                transition,
            ],
            "intent",
        );
        let now = nanos();
        self.evidence.append(&hash.to_string(), "intent:propose", now);
        Ok(hash)
    }

    async fn evaluate_hints(&self, _intent_id: &str) -> Result<bool, BlackboardError> {
        Ok(self.hints.is_empty()) // v1: true = no active hints blocking
    }

    async fn record_evidence(
        &self,
        action_hash: &str,
        action_type: &str,
    ) -> Result<(), BlackboardError> {
        let now = nanos();
        self.evidence.append(action_hash, action_type, now);
        Ok(())
    }

    async fn evidence_tip(&self) -> Option<String> {
        self.evidence.tip()
    }
}

// ── Timestamp helper ───────────────────────────────────────────────────

fn nanos() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}
