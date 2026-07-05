// ── FIH-specific contract layer ────────────────────────────────────────
//
// Composes core contract primitives into FIH-specific defaults.
// Governance flow belongs HERE, not in storage/core/store.rs.
// Storage core is a pure execution unit — contract layer adds gating.

use nexus_model::error::BlackboardError;
use nexus_model::fih::{Fact, FihHash};

use crate::contract::core::{GovernanceGate, HintRule};
use crate::io::FileIo;
use crate::storage::core::FihStorage;
use nexus_model::{AsyncFactCapable, AsyncIntentCapable, GovernanceCapable};

/// Register the default FIH schemas mapped by content mime_type.
pub fn register_default_fih_schemas(gate: &GovernanceGate) {
    gate.register_schema("text/plain", b"text");
    gate.register_schema("text/markdown", b"markdown");
    gate.register_schema("application/x-nex-calc-number", b"i64");
    gate.register_schema("application/octet-stream", b"blob");
}

/// Governed fact submission: admit → submit.
///
/// Schema defaults to the fact's content mime_type. Call this instead
/// of raw `submit_fact()` when governance is active.
pub async fn submit_gov_fact<I: FileIo>(
    storage: &FihStorage<I>,
    fact: &Fact,
    schema: Option<&str>,
) -> Result<FihHash, BlackboardError> {
    let schema = schema.unwrap_or(&fact.content.mime_type);
    storage.admit_fact(schema, &fact.content.data)?;
    storage.submit_fact(fact).await
}

/// Governed intent conclusion: check hints → conclude.
pub async fn conclude_gov_intent<I: FileIo>(
    storage: &FihStorage<I>,
    intent_id: &str,
    result: &str,
) -> Result<nexus_model::fih::Fact, BlackboardError> {
    if let Ok(numeric) = result.trim().parse::<i64>() {
        storage.check_hints(numeric)?;
    }
    storage.conclude_intent(intent_id, result).await
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
