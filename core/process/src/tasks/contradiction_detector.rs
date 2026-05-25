// nexus-process — Contradiction detector: finds conflicting claims across documents.
//
// When two Facts from different documents share the same topic but express
// different positions, the detector creates a "resolve-contradiction" Intent.
// This implements the stigmergy "many iterations" heuristic: each tension
// discovered becomes an exploration opportunity.
//
// Position comparison is string-based: any two different position values on
// the same topic are treated as a contradiction to resolve. The system does
// not judge which position is correct — it flags the tension for research.

use super::{TaskHandler, TaskOutput};
use nexus_model::{BoardState, Fact, FihHash, Intent};
use std::collections::{HashMap, HashSet};

/// Detects pairs of Facts that share a topic but hold different positions.
pub struct ContradictionDetector {
    /// (topic, position_a, position_b) tuples already flagged.
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

fn topic_of(fact: &Fact) -> Option<&str> {
    fact.content.get("topic")?.as_str()
}

fn position_of(fact: &Fact) -> Option<&str> {
    fact.content.get("position")?.as_str()
}

impl TaskHandler for ContradictionDetector {
    fn name(&self) -> &str {
        "contradiction-detector"
    }

    fn orient(&mut self, state: &BoardState) -> TaskOutput {
        // Group facts by topic → map of (position → list of facts)
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

        let mut output = TaskOutput::default();

        // For each topic with multiple positions, flag the contradiction
        for (topic, positions) in &by_topic {
            let pos_keys: Vec<&&str> = positions.keys().collect();
            if pos_keys.len() < 2 {
                continue;
            }

            // Create an intent for each pair of differing positions
            for i in 0..pos_keys.len() {
                for j in (i + 1)..pos_keys.len() {
                    let pos_a = pos_keys[i];
                    let pos_b = pos_keys[j];

                    // Sort to get canonical key
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

                    let facts_a = &positions[pos_a];
                    let facts_b = &positions[pos_b];
                    let from_facts: Vec<String> = facts_a
                        .iter()
                        .chain(facts_b.iter())
                        .map(|f| f.id.0.clone())
                        .collect();

                    let origins_a: Vec<&str> = facts_a.iter().map(|f| f.origin.as_str()).collect();
                    let origins_b: Vec<&str> = facts_b.iter().map(|f| f.origin.as_str()).collect();

                    let desc = format!(
                        "Resolve contradiction on '{}': [{}] (from {}) vs [{}] (from {})",
                        topic,
                        pa,
                        origins_a.join(", "),
                        pb,
                        origins_b.join(", ")
                    );

                    let intent = Intent {
                        id: FihHash::new(&[topic, pa, pb], "contradiction"),
                        from_facts,
                        description: desc,
                        creator: "contradiction-detector".into(),
                        worker: None,
                        to_fact_id: None,
                        last_heartbeat_at: None,
                        created_at: None,
                        concluded_at: None,
                    };
                    output.intents.push(intent);
                }
            }
        }

        output
    }
}
