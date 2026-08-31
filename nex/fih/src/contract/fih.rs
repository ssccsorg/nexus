// ── FIH-specific contract layer ────────────────────────────────────────
//
// The FIH contract layer orchestrates governance primitives into a
// coherent pipeline: gate to storage to evidence. Apps that want governed
// FIH operations create a `FihContract` and call its methods.
// Apps without governance use FihStorage directly.
//
// The contract layer is a semantic model plus storage specification: it
// never reads wall-clock time or executes async I/O itself. Time comes
// from a `Now` implementation injected by the caller (the host clock on
// native, an EpochClock on MCU), and I/O goes through the `FileIo` trait
// implemented by chton. This keeps the layer std-free and environment-
// agnostic.

use crate::error::BlackboardError;
use crate::{CoordId, Fact};
use alloc::boxed::Box;
use alloc::string::ToString;

use crate::contract::core::{EvidenceChain, GovernanceGate, HintEngine, HintRule};
use crate::core::store::FihStorage;
use crate::io::FileIo;
use crate::{AsyncFactCapable, AsyncIntentCapable};
use nex_core::Now;

/// Bundled FIH contract: gate + hints + evidence.
///
/// Owns the three governance primitives and provides governed
/// fact/intent operations. Takes storage as a parameter and time from an
/// injected `Now` clock.
pub struct FihContract {
    pub gate: GovernanceGate,
    pub hints: HintEngine,
    pub evidence: EvidenceChain,
    /// Wall-clock source for evidence timestamps. Injected by the caller
    /// so the contract layer never depends on `std::time`.
    clock: Box<dyn Now + Send + Sync>,
}

impl FihContract {
    /// Create a contract with the given wall-clock source.
    pub fn with_clock(clock: Box<dyn Now + Send + Sync>) -> Self {
        Self {
            gate: GovernanceGate::new(),
            hints: HintEngine::new(),
            evidence: EvidenceChain::new(),
            clock,
        }
    }

    /// Create a contract with the host system clock.
    #[cfg(feature = "std")]
    pub fn new() -> Self {
        Self::with_clock(Box::new(nex_core::SystemClock))
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
    ) -> Result<CoordId, BlackboardError> {
        let schema = schema.unwrap_or(&fact.content.mime_type);
        self.gate
            .admit(schema, &fact.content.data)
            .map_err(|e| BlackboardError::Forbidden(e.to_string()))?;
        let hash = storage.submit_fact(fact).await?;
        let ts = self.clock.now_nanos();
        self.evidence.append(&hash.to_string(), "fact:submit", ts);
        Ok(hash)
    }

    /// Governed intent conclusion: hints.check to storage.conclude to evidence.
    pub async fn conclude_intent<I: FileIo>(
        &self,
        storage: &FihStorage<I>,
        intent_id: &str,
        result: &str,
    ) -> Result<crate::Fact, BlackboardError> {
        if let Ok(numeric) = result.trim().parse::<i64>() {
            self.hints
                .check_numeric(numeric)
                .map_err(BlackboardError::Forbidden)?;
        }
        let fact = storage.conclude_intent(intent_id, result).await?;
        let ts = self.clock.now_nanos();
        self.evidence.append(intent_id, "intent:conclude", ts);
        Ok(fact)
    }
}

impl Default for FihContract {
    fn default() -> Self {
        #[cfg(feature = "std")]
        {
            Self::new()
        }
        #[cfg(not(feature = "std"))]
        {
            // no_std targets must inject a real clock via `with_clock`;
            // the default is a zero baseline so the type still constructs.
            struct ZeroClock;
            impl nex_core::Monotonic for ZeroClock {
                fn elapsed_nanos(&self) -> u64 {
                    0
                }
            }
            Self::with_clock(Box::new(nex_core::EpochClock::new(0, ZeroClock)))
        }
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
