// nexus-process — New-document analyzer: evaluates incoming facts against existing knowledge.
//
// When new Facts appear, the analyzer compares each against the existing
// corpus and records observations as Facts:
//   +factor (support):     same topic, same position
//   -factor (challenge):   same topic, different position
//   gap (explore):         new topic not in existing corpus
//
// Implements: DetectionCapable (standalone, no marker trait)
//
// Stigmergy principle: each new document's relationship to existing
// knowledge is an observed fact. What to do about challenges or gaps
// is for agents to decide in later iterations.

use super::common::{position_of, topic_of};
use nexus_model::{BoardState, DetectionCapable, DetectionOutput, Fact, FihHash};
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

            let (factor, detail) = if let Some(existing_pos_set) = existing_positions.get(topic) {
                if existing_pos_set.contains(position) {
                    (
                        "+factor",
                        format!("supports existing position [{}]", position),
                    )
                } else {
                    let existing: Vec<&str> = existing_pos_set.iter().copied().collect();
                    (
                        "-factor",
                        format!(
                            "claims [{}], but existing holds [{}]",
                            position,
                            existing.join(", ")
                        ),
                    )
                }
            } else {
                ("gap", format!("new topic '{}'", topic))
            };

            output.facts.push(Fact {
                id: FihHash::new(&[tid, factor], "doc-analysis"),
                origin: "new-document-analyzer".into(),
                content: serde_json::json!({
                    "type": "doc_analysis",
                    "factor": factor,
                    "topic": topic,
                    "claim": claim_text,
                    "source": fact.origin,
                    "detail": detail,
                }),
                creator: "new-document-analyzer".into(),
            });

            existing_positions
                .entry(topic)
                .or_default()
                .insert(position);
        }

        output
    }
}
