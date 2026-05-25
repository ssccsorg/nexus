// nexus-process — Task handler trait for stigmergy detectors.
//
// Each registered task implements `TaskHandler` and is called by the
// scheduler during the Orient phase of the OODA loop. Tasks examine
// the current `BoardState` and optionally submit new Intents or Facts.

use nexus_model::{BoardState, Fact, Intent};

/// A stigmergy task that runs during each OODA iteration's Orient phase.
///
/// The task receives a snapshot of the current `BoardState` and returns
/// a list of Intents and Facts to submit. The scheduler applies them
/// during the Decide/Act phases.
pub trait TaskHandler {
    /// Human-readable name for logging/debugging.
    fn name(&self) -> &str;

    /// Examine the current state and produce new FIH primitives.
    /// Called every OODA tick.
    fn orient(&mut self, state: &BoardState) -> TaskOutput;
}

pub mod contradiction_detector;
pub mod gap_detector;
pub mod new_document_analyzer;

/// Output from a task handler: Intents and Facts to submit.
#[derive(Debug, Default)]
pub struct TaskOutput {
    /// Intents to submit via `Blackboard::submit_intent()`.
    pub intents: Vec<Intent>,
    /// Facts to submit via `Blackboard::submit_fact()`.
    pub facts: Vec<Fact>,
}
