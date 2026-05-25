// nexus-process — Contradiction detector: finds conflicting claims across documents.
//
// When two Facts from different documents share the same topic but express
// different positions, the detector creates a "resolve-contradiction" Intent.
//
// Implements: DetectionCapable + ContradictionDetection (from nexus-model)

use nexus_model::{
    BoardState, ContradictionDetection, DetectionCapable, DetectionOutput, Fact, FihHash, Intent,
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

fn topic_of(fact: &Fact) -> Option<&str> {
    fact.content.get("topic")?.as_str()
}

fn position_of(fact: &Fact) -> Option<&str> {
    fact.content.get("position")?.as_str()
}

impl DetectionCapable for ContradictionDetector {
    fn name(&self) -> &str {
        "contradiction-detector"
    }

    fn orient(&mut self, state: &BoardState) -> DetectionOutput {
        let mut by_topic: HashMap<&str, HashMap<&str, Vec<&Fact>>> = HashMap::new();
        for fact in &state.facts {
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

                    let from_facts: Vec<String> = positions[pos_a]
                        .iter()
                        .chain(positions[pos_b].iter())
                        .map(|f| f.id.0.clone())
                        .collect();

                    let origins_a: Vec<&str> =
                        positions[pos_a].iter().map(|f| f.origin.as_str()).collect();
                    let origins_b: Vec<&str> =
                        positions[pos_b].iter().map(|f| f.origin.as_str()).collect();

                    output.intents.push(Intent {
                        id: FihHash::new(&[topic, pa, pb], "contradiction"),
                        from_facts,
                        description: format!(
                            "Resolve contradiction on '{}': [{}] (from {}) vs [{}] (from {})",
                            topic,
                            pa,
                            origins_a.join(", "),
                            pb,
                            origins_b.join(", ")
                        ),
                        creator: "contradiction-detector".into(),
                        worker: None,
                        to_fact_id: None,
                        last_heartbeat_at: None,
                        created_at: None,
                        concluded_at: None,
                    });
                }
            }
        }

        output
    }
}
