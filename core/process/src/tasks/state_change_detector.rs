// nexus-process — State change detector: Cairn-style ReasonCheckpoint pattern.
//
// Detects when the Blackboard state has changed sufficiently to warrant
// a new "reason" analysis. Uses count-based checkpoints (snapshot-safe).
//
// Implements: DetectionCapable + StateChangeDetection (from nexus-model)
// This is the mechanism Cairn's _reason_trigger uses — proven across
// 54/54 penetration testing challenges.

use nexus_model::{
    BoardState, DetectionCapable, DetectionCheckpoint, DetectionOutput, FihHash, Intent,
    StateChangeDetection,
};

pub struct StateChangeDetector {
    checkpoint: Option<DetectionCheckpoint>,
}

impl StateChangeDetector {
    pub fn new() -> Self {
        Self { checkpoint: None }
    }
}

impl Default for StateChangeDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl StateChangeDetection for StateChangeDetector {
    fn to_checkpoint(&self) -> Option<DetectionCheckpoint> {
        self.checkpoint.clone()
    }

    fn from_checkpoint(checkpoint: DetectionCheckpoint) -> Self {
        Self {
            checkpoint: Some(checkpoint),
        }
    }
}

impl DetectionCapable for StateChangeDetector {
    fn name(&self) -> &str {
        "state-change-detector"
    }

    fn orient(&mut self, state: &BoardState) -> DetectionOutput {
        let current_facts = state.facts.len();
        let current_open = state
            .intents
            .iter()
            .filter(|i| i.to_fact_id.is_none())
            .count();

        let mut output = DetectionOutput::default();

        let Some(ref checkpoint) = self.checkpoint else {
            self.checkpoint = Some(DetectionCheckpoint {
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

        output.intents.push(Intent {
            id: FihHash::new(&[&triggers.join(",")], "reason"),
            from_facts: Vec::new(),
            description: format!("Reason: state changed ({})", triggers.join(", ")),
            creator: "state-change-detector".into(),
            worker: None,
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            concluded_at: None,
        });

        self.checkpoint = Some(DetectionCheckpoint {
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
        assert!(output.intents.is_empty());
    }

    #[test]
    fn detects_fact_increase() {
        let mut detector = StateChangeDetector::new();
        let state1 = BoardState {
            facts: vec![make_fact("f1", "a")],
            intents: Vec::new(),
            hints: Vec::new(),
        };
        detector.orient(&state1);

        let state2 = BoardState {
            facts: vec![make_fact("f1", "a"), make_fact("f2", "b")],
            intents: Vec::new(),
            hints: Vec::new(),
        };
        let output = detector.orient(&state2);
        assert_eq!(output.intents.len(), 1);
        assert!(output.intents[0].description.contains("facts:1->2"));
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
        assert_eq!(output.intents.len(), 1);
        assert!(output.intents[0].description.contains("open_intents"));
    }

    #[test]
    fn no_change_no_intent() {
        let mut detector = StateChangeDetector::new();
        let state = BoardState {
            facts: vec![make_fact("f1", "a")],
            intents: Vec::new(),
            hints: Vec::new(),
        };
        detector.orient(&state);
        let output = detector.orient(&state);
        assert!(output.intents.is_empty());
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

        let mut restored = StateChangeDetector::from_checkpoint(cp);
        let output = restored.orient(&state);
        assert!(output.intents.is_empty());
    }
}
