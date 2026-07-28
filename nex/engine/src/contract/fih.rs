// ── FIH-specific contract layer ────────────────────────────────────────
//
// The FIH contract layer orchestrates governance primitives into a
// coherent pipeline: gate to storage to evidence. Apps that want governed
// FIH operations create a `FihContract` and call its methods.
// Apps without governance use FihStorage directly.

use std::time::SystemTime;

use nexus_model::error::BlackboardError;
use nexus_model::fih::{Fact, FihHash};

use crate::contract::core::{EvidenceChain, GovernanceGate, HintEngine, HintRule};
use crate::io::FileIo;
use crate::storage::core::FihStorage;
use nexus_model::{AsyncFactCapable, AsyncIntentCapable};

/// Bundled FIH contract: gate + hints + evidence.
///
/// Owns the three governance primitives and provides governed
/// fact/intent operations. Takes storage as a parameter.
pub struct FihContract {
    pub gate: GovernanceGate,
    pub hints: HintEngine,
    pub evidence: EvidenceChain,
}

impl FihContract {
    pub fn new() -> Self {
        Self {
            gate: GovernanceGate::new(),
            hints: HintEngine::new(),
            evidence: EvidenceChain::new(),
        }
    }

    /// Register default FIH schemas (text/markdown, text/plain, etc.).
    pub fn register_default_schemas(&self) {
        register_default_fih_schemas(&self.gate);
    }

    /// Governed fact submission: gate.admit to storage.submit to evidence.
    pub async fn submit_fact<I: FileIo>(
        &self,
        storage: &FihStorage<I>,
        fact: &Fact,
        schema: Option<&str>,
    ) -> Result<FihHash, BlackboardError> {
        let schema = schema.unwrap_or(&fact.content.mime_type);
        self.gate
            .admit(schema, &fact.content.data)
            .map_err(|e| BlackboardError::Forbidden(e.to_string()))?;
        let hash = storage.submit_fact(fact).await?;
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        self.evidence.append(&hash.to_string(), "fact:submit", ts);
        Ok(hash)
    }

    /// Governed intent conclusion: hints.check to storage.conclude to evidence.
    pub async fn conclude_intent<I: FileIo>(
        &self,
        storage: &FihStorage<I>,
        intent_id: &str,
        result: &str,
    ) -> Result<nexus_model::fih::Fact, BlackboardError> {
        if let Ok(numeric) = result.trim().parse::<i64>() {
            self.hints
                .check_numeric(numeric)
                .map_err(BlackboardError::Forbidden)?;
        }
        let fact = storage.conclude_intent(intent_id, result).await?;
        let ts = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        self.evidence.append(intent_id, "intent:conclude", ts);
        Ok(fact)
    }
}

impl Default for FihContract {
    fn default() -> Self {
        Self::new()
    }
}

/// Register the default FIH schemas.
pub fn register_default_fih_schemas(gate: &GovernanceGate) {
    gate.register_schema("text/plain", b"text");
    gate.register_schema("text/markdown", b"markdown");
    gate.register_schema("application/x-nex-calc-number", b"i64");
    gate.register_schema("application/octet-stream", b"blob");
}

/// Standard FIH constraint factories.
pub mod constraints {
    use super::HintRule;

    pub fn positive() -> HintRule {
        HintRule::Positive
    }
    pub fn even() -> HintRule {
        HintRule::Even
    }
    pub fn gt(n: i64) -> HintRule {
        HintRule::Gt(n)
    }
    pub fn lt(n: i64) -> HintRule {
        HintRule::Lt(n)
    }
    pub fn non_negative() -> HintRule {
        HintRule::Gt(-1)
    }
    pub fn eq(n: i64) -> HintRule {
        HintRule::Eq(n)
    }
}
