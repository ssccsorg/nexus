// nexus-process — State change detector: Cairn-style ReasonCheckpoint pattern.
//
// Detects when the Blackboard state has changed sufficiently to warrant
// attention. Records state transitions as Facts — immutable observations.
//
// Implements: DetectionCapable + StateChangeDetection (from nexus-model)
//
// Stigmergy principle: state change is an observed fact. What to do about
// it is a separate decision for agents in later iterations.

use nexus_model::{
    BoardState, DetectionCapable, DetectionCheckpoint, DetectionOutput, Fact, FihHash,
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

        output.facts.push(Fact {
            id: FihHash::new(&[&triggers.join(",")], "state-change"),
            origin: "state-change-detector".into(),
            content: serde_json::to_string(&serde_json::json!({
                "type": "state_change",
                "triggers": triggers,
                "prev_fact_count": checkpoint.fact_count,
                "curr_fact_count": current_facts,
                "prev_open_intents": checkpoint.open_intent_count,
                "curr_open_intents": current_open,
            }))
            .unwrap()
            .into(),
            creator: "state-change-detector".into(),
        });

        self.checkpoint = Some(DetectionCheckpoint {
            fact_count: current_facts,
            open_intent_count: current_open,
        });

        output
    }

    fn snapshot_state(&self) -> Option<nexus_model::Content> {
        self.checkpoint.as_ref().map(|cp| {
            nexus_model::Content::Text(
                serde_json::json!({
                    "fact_count": cp.fact_count,
                    "open_intent_count": cp.open_intent_count,
                })
                .to_string(),
            )
        })
    }

    fn restore_state(&mut self, state: nexus_model::Content) {
        let json = state
            .as_str()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
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

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_model::FihHash;

    fn make_fact(id: &str, origin: &str) -> Fact {
        Fact {
            id: FihHash(id.to_string()),
            origin: origin.to_string(),
            content: serde_json::to_string(&serde_json::json!({"topic": "test"}))
                .unwrap()
                .into(),
            creator: "test".into(),
        }
    }

    #[test]
    fn first_call_silent() {
        let mut d = StateChangeDetector::new();
        let s = BoardState {
            facts: vec![make_fact("f1", "a")],
            intents: vec![],
            hints: vec![],
        };
        let o = d.orient(&s);
        assert!(o.facts.is_empty());
    }

    #[test]
    fn detects_fact_increase() {
        let mut d = StateChangeDetector::new();
        d.orient(&BoardState {
            facts: vec![make_fact("f1", "a")],
            intents: vec![],
            hints: vec![],
        });
        let o = d.orient(&BoardState {
            facts: vec![make_fact("f1", "a"), make_fact("f2", "b")],
            intents: vec![],
            hints: vec![],
        });
        assert_eq!(o.facts.len(), 1);
        assert!(
            serde_json::from_str::<serde_json::Value>(o.facts[0].content.as_str().unwrap_or(""))
                .unwrap_or(serde_json::Value::Null)["type"]
                .as_str()
                == Some("state_change")
        );
    }

    #[test]
    fn no_change_no_fact() {
        let mut d = StateChangeDetector::new();
        let s = BoardState {
            facts: vec![make_fact("f1", "a")],
            intents: vec![],
            hints: vec![],
        };
        d.orient(&s);
        let o = d.orient(&s);
        assert!(o.facts.is_empty());
    }
}
