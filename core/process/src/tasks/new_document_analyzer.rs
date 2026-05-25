// nexus-process — New-document analyzer: evaluates incoming facts against existing knowledge.
//
// When new Facts appear on the Blackboard, the analyzer compares each
// against the existing corpus:
//   +factor (support):     same topic, same position
//   -factor (challenge):   same topic, different position
//   gap (explore):         new topic not in existing corpus
//
// Implements: DetectionCapable (standalone, no marker trait)

use super::common::{position_of, topic_of};
use nexus_model::{BoardState, DetectionCapable, DetectionOutput, Fact, FihHash, Intent};
use std::collections::{HashMap, HashSet};

pub struct NewDocumentAnalyzer {
    seen_ids: HashSet<String>,
}

impl NewDocumentAnalyzer {
    pub fn new() -> Self {
        Self {
            seen_ids: HashSet::new(),
        }
    }

    pub fn with_baseline(ids: impl IntoIterator<Item = String>) -> Self {
        Self {
            seen_ids: ids.into_iter().collect(),
        }
    }
}

impl Default for NewDocumentAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl DetectionCapable for NewDocumentAnalyzer {
    fn name(&self) -> &str {
        "new-document-analyzer"
    }

    fn orient(&mut self, state: &BoardState) -> DetectionOutput {
        let new_facts: Vec<&Fact> = state
            .facts
            .iter()
            .filter(|f| !self.seen_ids.contains(&f.id.0))
            .collect();

        if new_facts.is_empty() {
            return DetectionOutput::default();
        }

        let existing_facts: Vec<&Fact> = state
            .facts
            .iter()
            .filter(|f| self.seen_ids.contains(&f.id.0))
            .collect();

        let mut existing_positions: HashMap<&str, HashSet<&str>> = HashMap::new();
        for f in &existing_facts {
            if let (Some(topic), Some(pos)) = (topic_of(f), position_of(f)) {
                existing_positions.entry(topic).or_default().insert(pos);
            }
        }

        let mut output = DetectionOutput::default();

        for fact in &new_facts {
            let tid = &fact.id.0;
            self.seen_ids.insert(tid.clone());

            let Some(topic) = topic_of(fact) else {
                continue;
            };
            let Some(position) = position_of(fact) else {
                continue;
            };

            let claim_text = fact
                .content
                .get("claim")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if let Some(existing_pos_set) = existing_positions.get(topic) {
                if existing_pos_set.contains(position) {
                    output.intents.push(Intent {
                        id: FihHash::new(&[tid, "support"], "intent"),
                        from_facts: vec![tid.clone()],
                        description: format!(
                            "+factor on '{}': '{}' from {} supports existing position [{}]",
                            topic, claim_text, fact.origin, position
                        ),
                        creator: "new-document-analyzer".into(),
                        worker: None,
                        to_fact_id: None,
                        last_heartbeat_at: None,
                        created_at: None,
                        concluded_at: None,
                    });
                } else {
                    let existing: Vec<&str> = existing_pos_set.iter().map(|s| *s).collect();
                    output.intents.push(Intent {
                        id: FihHash::new(&[tid, "challenge"], "intent"),
                        from_facts: vec![tid.clone()],
                        description: format!(
                            "-factor on '{}': '{}' from {} claims [{}], but existing holds [{}]",
                            topic,
                            claim_text,
                            fact.origin,
                            position,
                            existing.join(", ")
                        ),
                        creator: "new-document-analyzer".into(),
                        worker: None,
                        to_fact_id: None,
                        last_heartbeat_at: None,
                        created_at: None,
                        concluded_at: None,
                    });
                }
            } else {
                output.intents.push(Intent {
                    id: FihHash::new(&[tid, "new-topic"], "intent"),
                    from_facts: vec![tid.clone()],
                    description: format!(
                        "Gap discovered: new topic '{}' from {} — '{}'",
                        topic, fact.origin, claim_text
                    ),
                    creator: "new-document-analyzer".into(),
                    worker: None,
                    to_fact_id: None,
                    last_heartbeat_at: None,
                    created_at: None,
                    concluded_at: None,
                });
            }

            existing_positions
                .entry(topic)
                .or_default()
                .insert(position);
        }

        output
    }
}
