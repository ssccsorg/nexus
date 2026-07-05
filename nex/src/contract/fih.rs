// ── FIH-specific contract layer ────────────────────────────────────────
//
// Composes core contract primitives into FIH-specific defaults.
// nex-apps that use FIH can compose these out of the box; apps using
// different paradigms define their own contracts via core primitives.
//
// Follows the same pattern as storage/fih.rs (FihBlackboard) and
// storage/semantic/fih.rs (FihRecordLoad): paradigm-specific
// composition of core primitives.

use crate::contract::core::{GovernanceGate, HintRule};

/// Register the default FIH schemas for number, text, and blob data.
///
/// nex-calc and similar numeric apps should call this during setup
/// to ensure standard schemas are available for gate admission.
pub fn register_default_fih_schemas(gate: &GovernanceGate) {
    gate.register_schema("number", b"i64");
    gate.register_schema("text", b"text/plain");
    gate.register_schema("blob", b"application/octet-stream");
}

/// Standard FIH constraint factories.
pub mod constraints {
    use super::HintRule;

    /// Result must be positive (> 0).
    pub fn positive() -> HintRule {
        HintRule::Positive
    }

    /// Result must be even.
    pub fn even() -> HintRule {
        HintRule::Even
    }

    /// Result must be greater than `n`.
    pub fn gt(n: i64) -> HintRule {
        HintRule::Gt(n)
    }

    /// Result must be less than `n`.
    pub fn lt(n: i64) -> HintRule {
        HintRule::Lt(n)
    }

    /// Result must be non-negative (>= 0).
    pub fn non_negative() -> HintRule {
        HintRule::Gt(-1)
    }

    /// Result must equal `n`.
    pub fn eq(n: i64) -> HintRule {
        HintRule::Eq(n)
    }
}
