// nexus-process — New-document analyzer: evaluates incoming facts against existing knowledge.
//
// When new Facts appear on the Blackboard (facts not seen in the previous
// tick), the analyzer compares each against the existing corpus:
//
//   +factor (support):     same topic, same position → strengthens existing knowledge
//   -factor (challenge):   same topic, different position → tension to resolve
//   gap (explore):         new topic not in existing corpus → exploration frontier
//
// This is a stateful detector: it tracks which fact IDs it has already
// analyzed so that each new fact is analyzed exactly once.

use super::{TaskHandler, TaskOutput};
use nexus_model::{BoardState, Fact, FihHash, Intent};
use std::collections::{HashMap, HashSet};

/// Analyzes newly arrived Facts against the existing knowledge base.
pub struct NewDocumentAnalyzer {
    /// Fact IDs already analyzed in previous ticks.
    seen_ids: HashSet<String>,
}

impl NewDocumentAnalyzer {
    pub fn new() -> Self {
        Self {
            seen_ids: HashSet::new(),
        }
    }

    /// Create an analyzer that treats the given fact IDs as already-seen
    /// (baseline). Only facts NOT in this set will be analyzed as "new."
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

fn topic_of(fact: &Fact) -> Option<&str> {
    fact.content.get("topic")?.as_str()
}

fn position_of(fact: &Fact) -> Option<&str> {
    fact.content.get("position")?.as_str()
}

impl TaskHandler for NewDocumentAnalyzer {
    fn name(&self) -> &str {
        "new-document-analyzer"
    }

    fn orient(&mut self, state: &BoardState) -> TaskOutput {
        // Identify new facts (not yet seen)
        let new_facts: Vec<&Fact> = state
            .facts
            .iter()
            .filter(|f| !self.seen_ids.contains(&f.id.0))
            .collect();

        if new_facts.is_empty() {
            return TaskOutput::default();
        }

        // Build existing knowledge index: topic → set of positions
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

        let mut output = TaskOutput::default();

        for fact in &new_facts {
            let tid = &fact.id.0;
            self.seen_ids.insert(tid.clone());

            let topic = match topic_of(fact) {
                Some(t) => t,
                None => continue,
            };
            let position = match position_of(fact) {
                Some(p) => p,
                None => continue,
            };

            if let Some(existing_pos_set) = existing_positions.get(topic) {
                if existing_pos_set.contains(position) {
                    // +factor: supports existing knowledge
                    let desc = format!(
                        "+factor on '{}': '{}' from {} supports existing position [{}]",
                        topic,
                        fact.content
                            .get("claim")
                            .and_then(|v| v.as_str())
                            .unwrap_or(""),
                        fact.origin,
                        position
                    );
                    let intent = Intent {
                        id: FihHash::new(&[tid, "support"], "intent"),
                        from_facts: vec![tid.clone()],
                        description: desc,
                        creator: "new-document-analyzer".into(),
                        worker: None,
                        to_fact_id: None,
                        last_heartbeat_at: None,
                        created_at: None,
                        concluded_at: None,
                    };
                    output.intents.push(intent);
                } else {
                    // -factor: challenges existing knowledge
                    let existing: Vec<&str> = existing_pos_set.iter().map(|s| *s).collect();
                    let desc = format!(
                        "-factor on '{}': '{}' from {} claims [{}], but existing corpus holds [{}]",
                        topic,
                        fact.content
                            .get("claim")
                            .and_then(|v| v.as_str())
                            .unwrap_or(""),
                        fact.origin,
                        position,
                        existing.join(", ")
                    );
                    let intent = Intent {
                        id: FihHash::new(&[tid, "challenge"], "intent"),
                        from_facts: vec![tid.clone()],
                        description: desc,
                        creator: "new-document-analyzer".into(),
                        worker: None,
                        to_fact_id: None,
                        last_heartbeat_at: None,
                        created_at: None,
                        concluded_at: None,
                    };
                    output.intents.push(intent);
                }
            } else {
                // Gap: entirely new topic
                let desc = format!(
                    "Gap discovered: new topic '{}' from {} — '{}'",
                    topic,
                    fact.origin,
                    fact.content
                        .get("claim")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                );
                let intent = Intent {
                    id: FihHash::new(&[tid, "new-topic"], "intent"),
                    from_facts: vec![tid.clone()],
                    description: desc,
                    creator: "new-document-analyzer".into(),
                    worker: None,
                    to_fact_id: None,
                    last_heartbeat_at: None,
                    created_at: None,
                    concluded_at: None,
                };
                output.intents.push(intent);
            }

            // Update existing_positions for subsequent new facts in same batch
            existing_positions
                .entry(topic)
                .or_default()
                .insert(position);
        }

        output
    }
}
