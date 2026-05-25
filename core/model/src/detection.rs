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

use crate::fih::{BoardState, Fact, Intent};

/// Output from a detector's orient phase.
#[derive(Debug, Default)]
pub struct DetectionOutput {
    /// Intents to submit via `Blackboard::submit_intent()`.
    pub intents: Vec<Intent>,
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
#[derive(Debug, Clone, Default)]
pub struct DetectionCheckpoint {
    pub fact_count: usize,
    pub open_intent_count: usize,
}

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

/// Aggregate: a detector that provides all standard detection capabilities.
/// This is the recommended default for most projects.
pub trait FullDetection: GapDetection + ContradictionDetection + StateChangeDetection {}

// Blanket impl for any type that implements all three.
impl<T> FullDetection for T where T: GapDetection + ContradictionDetection + StateChangeDetection {}
