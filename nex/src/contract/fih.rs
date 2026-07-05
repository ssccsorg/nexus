// ── FIH-specific contract layer ────────────────────────────────────────
//
// Composes core contract primitives into FIH-specific defaults.
// Storage core is a pure execution unit — apps compose governance
// by calling these helpers with explicit primitives.

use std::time::SystemTime;

use nexus_model::error::BlackboardError;
use nexus_model::fih::{Fact, FihHash};

use crate::contract::core::{EvidenceChain, GovernanceGate, HintEngine, HintRule};
use crate::io::FileIo;
use crate::storage::core::FihStorage;
use nexus_model::{AsyncFactCapable, AsyncIntentCapable};

/// Register the default FIH schemas mapped by content mime_type.
pub fn register_default_fih_schemas(gate: &GovernanceGate) {
    gate.register_schema("text/plain", b"text");
    gate.register_schema("text/markdown", b"markdown");
    gate.register_schema("application/x-nex-calc-number", b"i64");
    gate.register_schema("application/octet-stream", b"blob");
}

/// Governed fact submission: admit → submit → evidence.
///
/// Apps call this with their own primitives. Apps without governance
/// call `storage.submit_fact()` directly — FihStorage is independent.
pub async fn submit_gov_fact<I: FileIo>(
    storage: &FihStorage<I>,
    gate: &GovernanceGate,
    evidence: &EvidenceChain,
    fact: &Fact,
    schema: Option<&str>,
) -> Result<FihHash, BlackboardError> {
    let schema = schema.unwrap_or(&fact.content.mime_type);
    gate.admit(schema, &fact.content.data)
        .map_err(|e| BlackboardError::Forbidden(e.to_string()))?;
    let hash = storage.submit_fact(fact).await?;
    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    evidence.append(&hash.to_string(), "fact:submit", ts);
    Ok(hash)
}

/// Governed intent conclusion: check hints → conclude → evidence.
pub async fn conclude_gov_intent<I: FileIo>(
    storage: &FihStorage<I>,
    hints: &HintEngine,
    evidence: &EvidenceChain,
    intent_id: &str,
    result: &str,
) -> Result<nexus_model::fih::Fact, BlackboardError> {
    if let Ok(numeric) = result.trim().parse::<i64>() {
        hints.check_numeric(numeric)
            .map_err(|e| BlackboardError::Forbidden(e))?;
    }
    let fact = storage.conclude_intent(intent_id, result).await?;
    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    evidence.append(intent_id, "intent:conclude", ts);
    Ok(fact)
}

/// Standard FIH constraint factories.
pub mod constraints {
    use super::HintRule;

    pub fn positive() -> HintRule { HintRule::Positive }
    pub fn even() -> HintRule { HintRule::Even }
    pub fn gt(n: i64) -> HintRule { HintRule::Gt(n) }
    pub fn lt(n: i64) -> HintRule { HintRule::Lt(n) }
    pub fn non_negative() -> HintRule { HintRule::Gt(-1) }
    pub fn eq(n: i64) -> HintRule { HintRule::Eq(n) }
}
