// Hint processing is not yet implemented in the detection layer.
// Hint governance and human feedback injection belong in a future
// agent-level module, not in automated detectors.

// nexus-model — Detection capability traits for stigmergy detectors.
//
// Following the same pattern as storage capability traits (StorageRead,
// FactCapable, FilterCapable, etc.), detection capabilities are split
// into fine-grained traits. Each detector implements only what it provides.
//
//   DetectionCapable             — base: name + orient (all detectors)
//    ├── GapDetection            — orphan/cross-origin gap discovery
//    ├── ContradictionDetection  — conflicting claims on same topic
//    └── StateChangeDetection    — count-based change triggers (Cairn pattern)
//
// This allows:
//   - Swappable detector backends (same as storage backends)
//   - Domain-specific custom detectors (legal, medical, hardware)
//   - Minimal implementations (only implement what you need)
//   - Future: EmbeddingSimilarityDetection, TemporalAnomalyDetection, etc.

use crate::fih::{BoardState, Content, Fact};
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

// `std::collections::HashMap` exists only under the std feature; alloc has
// no HashMap. The no_std path substitutes a BTreeMap, which satisfies the
// same contract (iteration order is unspecified for HashMap, so callers
// cannot rely on it).
#[cfg(not(feature = "std"))]
use alloc::collections::BTreeMap as HashMap;
#[cfg(feature = "std")]
use std::collections::HashMap;

/// Output from a detector's orient phase — detector emits Facts only.
#[derive(Debug, Default)]
pub struct DetectionOutput {
    /// Facts to submit via `Blackboard::submit_fact()`.
    pub facts: Vec<Fact>,
}

/// Base detection capability — every detector implements this.
pub trait DetectionCapable {
    /// Human-readable name for logging/debugging.
    fn name(&self) -> &str;

    /// Examine the current state and produce new FIH primitives.
    /// Called every OODA tick during the Orient phase.
    fn orient(&mut self, state: &BoardState) -> DetectionOutput;

    /// Export detector state for snapshot persistence.
    /// Default: None (stateless detector). Override to persist state.
    fn snapshot_state(&self) -> Option<Content> {
        None
    }

    /// Restore detector state from a previously saved snapshot.
    /// Default: no-op. Override to restore persisted state.
    fn restore_state(&mut self, _state: Content) {}
}

/// Detects gaps between facts: orphaned concepts, cross-origin clusters,
/// and cross-topic research frontiers.
///
/// Default implementation: `nexus-process::tasks::gap_detector::GapDetector`.
pub trait GapDetection: DetectionCapable {}

/// Detects contradictions: same topic, different position across documents.
///
/// Default implementation: `nexus-process::tasks::contradiction_detector::ContradictionDetector`.
pub trait ContradictionDetection: DetectionCapable {}

/// Snapshot-safe checkpoint for state change detection.
/// Only counts — no individual IDs — so it survives serialization.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DetectionCheckpoint {
    pub fact_count: usize,
    pub open_intent_count: usize,
}

/// Serialized detector states, keyed by detector name.
/// Stored in StorageSnapshot for cross-worker persistence.
pub type TaskStates = HashMap<String, Content>;

/// Detects when the Blackboard state has changed sufficiently to warrant
/// a new analysis cycle. Uses count-based checkpoints (Cairn pattern).
///
/// Default implementation: `nexus-process::tasks::state_change_detector::StateChangeDetector`.
pub trait StateChangeDetection: DetectionCapable {
    /// Export current checkpoint for snapshot serialization.
    fn to_checkpoint(&self) -> Option<DetectionCheckpoint>;

    /// Restore from a previously saved checkpoint.
    fn from_checkpoint(checkpoint: DetectionCheckpoint) -> Self
    where
        Self: Sized;
}
