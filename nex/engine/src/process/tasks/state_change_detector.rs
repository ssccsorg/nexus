// nexus-process — State change detector: Cairn-style ReasonCheckpoint pattern.
//
// Detects when the Blackboard state has changed sufficiently to warrant
// attention. Records state transitions as Facts — immutable observations.
//
// Implements: DetectionCapable + StateChangeDetection (from nexus-model)
//
// Stigmergy principle: state change is an observed fact. What to do about
// it is a separate decision for agents in later iterations.

use crate::helper::ContentJsonExt;
use nexus_model::{
    BoardState, Content, DetectionCapable, DetectionCheckpoint, DetectionOutput, Fact, FihHash,
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
        // Count only document-source facts (exclude all detector output)
        let current_facts = state
            .facts
            .iter()
            .filter(|f| {
                f.origin != "state-change-detector"
                    && f.origin != "gap-detector"
                    && f.origin != "contradiction-detector"
                    && f.origin != "new-document-analyzer"
            })
            .count();
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

        output.facts.push(Fact::new(
            FihHash::new(&[&triggers.join(",")], "state-change"),
            "state-change-detector".into(),
            Content::from_json(&serde_json::json!({
                "type": "state_change",
                "triggers": triggers,
                "prev_fact_count": checkpoint.fact_count,
                "curr_fact_count": current_facts,
                "prev_open_intents": checkpoint.open_intent_count,
                "curr_open_intents": current_open,
            })),
            "state-change-detector".into(),
        ));

        self.checkpoint = Some(DetectionCheckpoint {
            fact_count: current_facts,
            open_intent_count: current_open,
        });

        output
    }

    fn snapshot_state(&self) -> Option<Content> {
        self.checkpoint.as_ref().map(|cp| {
            Content::from_json(&serde_json::json!({
                "fact_count": cp.fact_count,
                "open_intent_count": cp.open_intent_count,
            }))
        })
    }

    fn restore_state(&mut self, state: Content) {
        let json = state
            .try_parse_json::<serde_json::Value>()
            .unwrap_or(serde_json::Value::Null);
        let fc = json.get("fact_count").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let oi = json
            .get("open_intent_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        self.checkpoint = Some(DetectionCheckpoint {
            fact_count: fc,
            open_intent_count: oi,
        });
    }
}
