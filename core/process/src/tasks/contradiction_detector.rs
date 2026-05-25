// nexus-process — Contradiction detector: finds conflicting claims across documents.
//
// Detects pairs of Facts that share a topic but hold different positions.
// Records contradictions as Facts — immutable observations of tension.
//
// Implements: DetectionCapable + ContradictionDetection (from nexus-model)
//
// Stigmergy principle: a contradiction is an observed fact about the
// knowledge state. Resolution is a separate act (Intent) by an agent.

use super::common::{position_of, topic_of};
use nexus_model::{
    BoardState, ContradictionDetection, DetectionCapable, DetectionOutput, Fact, FihHash,
};
use std::collections::{HashMap, HashSet};

pub struct ContradictionDetector {
    seen: HashSet<(String, String, String)>,
}

impl ContradictionDetector {
    pub fn new() -> Self {
        Self {
            seen: HashSet::new(),
        }
    }
}

impl Default for ContradictionDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl ContradictionDetection for ContradictionDetector {}

impl DetectionCapable for ContradictionDetector {
    fn name(&self) -> &str {
        "contradiction-detector"
    }

    fn orient(&mut self, state: &BoardState) -> DetectionOutput {
        // Only analyze document-source facts (skip detector output)
        let mut by_topic: HashMap<&str, HashMap<&str, Vec<&Fact>>> = HashMap::new();
        for fact in &state.facts {
            if fact.origin == "contradiction-detector"
                || fact.origin == "gap-detector"
                || fact.origin == "state-change-detector"
                || fact.origin == "new-document-analyzer"
            {
                continue;
            }
            if let (Some(topic), Some(position)) = (topic_of(fact), position_of(fact)) {
                by_topic
                    .entry(topic)
                    .or_default()
                    .entry(position)
                    .or_default()
                    .push(fact);
            }
        }

        let mut output = DetectionOutput::default();

        for (topic, positions) in &by_topic {
            let pos_keys: Vec<&&str> = positions.keys().collect();
            if pos_keys.len() < 2 {
                continue;
            }
            for i in 0..pos_keys.len() {
                for j in (i + 1)..pos_keys.len() {
                    let pos_a = pos_keys[i];
                    let pos_b = pos_keys[j];
                    let (pa, pb) = if pos_a < pos_b {
                        (*pos_a, *pos_b)
                    } else {
                        (*pos_b, *pos_a)
                    };
                    let key = (topic.to_string(), pa.to_string(), pb.to_string());
                    if self.seen.contains(&key) {
                        continue;
                    }
                    self.seen.insert(key.clone());

                    let origins_a: Vec<&str> =
                        positions[pos_a].iter().map(|f| f.origin.as_str()).collect();
                    let origins_b: Vec<&str> =
                        positions[pos_b].iter().map(|f| f.origin.as_str()).collect();

                    output.facts.push(Fact {
                        id: FihHash::new(&[topic, pa, pb], "contradiction"),
                        origin: "contradiction-detector".into(),
                        content: serde_json::json!({
                            "type": "contradiction",
                            "topic": topic,
                            "position_a": pa,
                            "position_b": pb,
                            "origins_a": origins_a,
                            "origins_b": origins_b,
                        }),
                        creator: "contradiction-detector".into(),
                    });
                }
            }
        }

        output
    }
}
