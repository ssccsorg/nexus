// nexus-process — State change detector: Cairn-style ReasonCheckpoint pattern.
//
// Detects when the Blackboard state has changed sufficiently to warrant
// a new "reason" analysis. Unlike NewDocumentAnalyzer (which tracks per-fact
// IDs and loses state on restart), this uses simple count-based checkpoints:
//
//   - Facts count increased → new knowledge arrived
//   - Intents opened or all concluded → stigmergy cycle completed
//
// This is the mechanism that Cairn's _reason_trigger uses — proven across
// 54/54 penetration testing challenges. It is snapshot-safe by design:
// only counts are compared, no individual IDs.

use super::{TaskHandler, TaskOutput};
use nexus_model::{BoardState, FihHash, Intent};

/// Snapshot-safe state checkpoint. Only counts, no IDs.
///
/// Designed to be serializable via serde (when trait extension adds it)
/// or via manual JSON construction for snapshot persistence.
#[derive(Debug, Clone, Default)]
pub struct ReasonCheckpoint {
    pub fact_count: usize,
    pub open_intent_count: usize,
}

/// Detects state changes and optionally creates a "reason" Intent.
///
/// On the first call (no checkpoint), it initializes silently.
/// On subsequent calls, if facts increased or open intents changed,
/// it creates a reason Intent to re-analyze the board.
pub struct StateChangeDetector {
    checkpoint: Option<ReasonCheckpoint>,
}

impl StateChangeDetector {
    pub fn new() -> Self {
        Self { checkpoint: None }
    }

    /// Restore from a previously saved checkpoint (e.g., after snapshot reload).
    pub fn from_checkpoint(checkpoint: ReasonCheckpoint) -> Self {
        Self {
            checkpoint: Some(checkpoint),
        }
    }

    /// Export current checkpoint for snapshot serialization.
    pub fn to_checkpoint(&self) -> Option<ReasonCheckpoint> {
        self.checkpoint.clone()
    }
}

impl Default for StateChangeDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskHandler for StateChangeDetector {
    fn name(&self) -> &str {
        "state-change-detector"
    }

    fn orient(&mut self, state: &BoardState) -> TaskOutput {
        let current_facts = state.facts.len();
        let current_open = state
            .intents
            .iter()
            .filter(|i| i.to_fact_id.is_none())
            .count();

        let mut output = TaskOutput::default();

        let Some(ref checkpoint) = self.checkpoint else {
            // First call: initialize silently
            self.checkpoint = Some(ReasonCheckpoint {
                fact_count: current_facts,
                open_intent_count: current_open,
            });
            return output;
        };

        let facts_changed = current_facts != checkpoint.fact_count;
        let open_intents_changed = current_open != checkpoint.open_intent_count;

        if !facts_changed && !open_intents_changed {
            return output;
        }

        // Build a reason description
        let mut triggers: Vec<String> = Vec::new();
        if facts_changed {
            triggers.push(format!(
                "facts:{}->{}",
                checkpoint.fact_count, current_facts
            ));
        }
        if open_intents_changed {
            triggers.push(format!(
                "open_intents:{}->{}",
                checkpoint.open_intent_count, current_open
            ));
        }

        let desc = format!("Reason: state changed ({})", triggers.join(", "));
        let intent = Intent {
            id: FihHash::new(&[&triggers.join(",")], "reason"),
            from_facts: Vec::new(), // reason is a meta-intent, not tied to specific facts
            description: desc,
            creator: "state-change-detector".into(),
            worker: None,
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            concluded_at: None,
        };
        output.intents.push(intent);

        // Update checkpoint AFTER creating the intent.
        // Add 1 to open_intent_count to account for the reason intent
        // we just created (it will be submitted to the blackboard by the
        // scheduler, increasing the count we'll see on the next tick).
        self.checkpoint = Some(ReasonCheckpoint {
            fact_count: current_facts,
            open_intent_count: current_open.saturating_add(output.intents.len()),
        });

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_model::{Fact, FihHash};

    fn make_fact(id: &str, origin: &str) -> Fact {
        Fact {
            id: FihHash(id.to_string()),
            origin: origin.to_string(),
            content: serde_json::json!({"topic": "test"}),
            creator: "test".into(),
        }
    }

    #[test]
    fn first_call_initializes_silently() {
        let mut detector = StateChangeDetector::new();
        let state = BoardState {
            facts: vec![make_fact("f1", "a")],
            intents: Vec::new(),
            hints: Vec::new(),
        };
        let output = detector.orient(&state);
        assert!(output.intents.is_empty(), "first call: no intents");
    }

    #[test]
    fn detects_fact_increase() {
        let mut detector = StateChangeDetector::new();
        let state1 = BoardState {
            facts: vec![make_fact("f1", "a")],
            intents: Vec::new(),
            hints: Vec::new(),
        };
        detector.orient(&state1); // initialize

        let state2 = BoardState {
            facts: vec![make_fact("f1", "a"), make_fact("f2", "b")],
            intents: Vec::new(),
            hints: Vec::new(),
        };
        let output = detector.orient(&state2);
        assert_eq!(output.intents.len(), 1, "fact increase triggers reason");
        assert!(
            output.intents[0].description.contains("facts:1->2"),
            "reason describes the change"
        );
    }

    #[test]
    fn detects_open_intent_change() {
        let mut detector = StateChangeDetector::new();
        let state1 = BoardState {
            facts: vec![make_fact("f1", "a")],
            intents: Vec::new(),
            hints: Vec::new(),
        };
        detector.orient(&state1);

        let intent = Intent {
            id: FihHash("i1".into()),
            from_facts: vec!["f1".into()],
            to_fact_id: None,
            description: "test".into(),
            creator: "test".into(),
            worker: None,
            last_heartbeat_at: None,
            created_at: None,
            concluded_at: None,
        };
        let state2 = BoardState {
            facts: vec![make_fact("f1", "a")],
            intents: vec![intent],
            hints: Vec::new(),
        };
        let output = detector.orient(&state2);
        assert_eq!(
            output.intents.len(),
            1,
            "open intent change triggers reason"
        );
        assert!(
            output.intents[0].description.contains("open_intents"),
            "reason describes open_intent change"
        );
    }

    #[test]
    fn no_change_no_intent() {
        let mut detector = StateChangeDetector::new();
        let state = BoardState {
            facts: vec![make_fact("f1", "a")],
            intents: Vec::new(),
            hints: Vec::new(),
        };
        detector.orient(&state); // initialize
        let output = detector.orient(&state); // no change
        assert!(output.intents.is_empty(), "no change: no reason intent");
    }

    #[test]
    fn checkpoint_roundtrip() {
        let mut detector = StateChangeDetector::new();
        let state = BoardState {
            facts: vec![make_fact("f1", "a"), make_fact("f2", "b")],
            intents: Vec::new(),
            hints: Vec::new(),
        };
        detector.orient(&state);

        let cp = detector.to_checkpoint().expect("has checkpoint");
        assert_eq!(cp.fact_count, 2);

        // Simulate snapshot reload
        let mut restored = StateChangeDetector::from_checkpoint(cp);
        let output = restored.orient(&state);
        assert!(output.intents.is_empty(), "restored: no false positive");
    }
}
