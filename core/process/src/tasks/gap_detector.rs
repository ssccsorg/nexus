// nexus-process — Gap detector: identifies unexplored concept pairs.
//
// Detects orphaned Facts at two levels:
//   1. Origin-based: Facts from the same origin with no grounding Intent.
//   2. Topic-based (cross-origin): Facts on the same topic from different
//      origins that have no connecting Intent — a cross-document research gap.
//
// This is the "many iterations" heuristic — eventually, every pair gets
// an Intent if it's interesting enough.

use super::{TaskHandler, TaskOutput};
use nexus_model::{BoardState, Fact, FihHash, Intent};
use std::collections::{HashMap, HashSet};

/// A gap detector that spots orphaned concepts (Facts with no Intent
/// grounding them to other Facts).
///
/// Tracks previously-synthesised (key) pairs to avoid submitting
/// duplicate Intents on successive OODA ticks.
pub struct GapDetector {
    /// Set of (origin, fact_count) tuples already synthesised.
    seen_origin: HashSet<(String, usize)>,
    /// Set of (topic, origin_a, origin_b) tuples already synthesised.
    seen_topic: HashSet<(String, String, String)>,
}

impl GapDetector {
    pub fn new() -> Self {
        Self {
            seen_origin: HashSet::new(),
            seen_topic: HashSet::new(),
        }
    }
}

impl Default for GapDetector {
    fn default() -> Self {
        Self::new()
    }
}

fn topic_of(fact: &Fact) -> Option<&str> {
    fact.content.get("topic")?.as_str()
}

impl TaskHandler for GapDetector {
    fn name(&self) -> &str {
        "gap-detector"
    }

    fn orient(&mut self, state: &BoardState) -> TaskOutput {
        // Find facts that are not referenced by any intent
        let referenced: HashSet<&str> = state
            .intents
            .iter()
            .flat_map(|i| i.from_facts.iter().map(|s| s.as_str()))
            .collect();

        let orphaned: Vec<&Fact> = state
            .facts
            .iter()
            .filter(|f| !referenced.contains(f.id.0.as_str()))
            .collect();

        if orphaned.is_empty() {
            return TaskOutput::default();
        }

        // ── Level 1: Origin-based grouping ──────────────────────────────
        let mut by_origin: HashMap<&str, Vec<&Fact>> = HashMap::new();
        for f in &orphaned {
            by_origin.entry(f.origin.as_str()).or_default().push(f);
        }

        let mut output = TaskOutput::default();

        for (origin, facts) in &by_origin {
            if facts.len() >= 2 {
                let key = ((*origin).to_string(), facts.len());
                if self.seen_origin.contains(&key) {
                    continue;
                }
                self.seen_origin.insert(key);

                let desc = format!("Synthesise {} orphaned facts from {}", facts.len(), origin);
                let intent = Intent {
                    id: FihHash::new(&[origin, "gap"], "intent"),
                    from_facts: facts.iter().map(|f| f.id.0.clone()).collect(),
                    description: desc,
                    creator: "gap-detector".into(),
                    worker: None,
                    to_fact_id: None,
                    last_heartbeat_at: None,
                    created_at: None,
                    concluded_at: None,
                };
                output.intents.push(intent);
            }
        }

        // ── Level 2: Topic-based cross-origin grouping ──────────────────
        let mut by_topic: HashMap<&str, HashMap<&str, Vec<&Fact>>> = HashMap::new();
        for f in &orphaned {
            if let Some(topic) = topic_of(f) {
                by_topic
                    .entry(topic)
                    .or_default()
                    .entry(f.origin.as_str())
                    .or_default()
                    .push(f);
            }
        }

        for (topic, origins) in &by_topic {
            let origin_keys: Vec<&&str> = origins.keys().collect();
            if origin_keys.len() < 2 {
                continue;
            }

            for i in 0..origin_keys.len() {
                for j in (i + 1)..origin_keys.len() {
                    let oa = origin_keys[i];
                    let ob = origin_keys[j];

                    let (oa_sorted, ob_sorted) = if oa < ob { (*oa, *ob) } else { (*ob, *oa) };

                    let key = (
                        topic.to_string(),
                        oa_sorted.to_string(),
                        ob_sorted.to_string(),
                    );
                    if self.seen_topic.contains(&key) {
                        continue;
                    }
                    self.seen_topic.insert(key);

                    let facts_a = &origins[oa];
                    let facts_b = &origins[ob];
                    let from_facts: Vec<String> = facts_a
                        .iter()
                        .chain(facts_b.iter())
                        .map(|f| f.id.0.clone())
                        .collect();

                    let desc = format!(
                        "Cross-origin gap on '{}': {} facts from {} ↔ {} facts from {}",
                        topic,
                        facts_a.len(),
                        oa,
                        facts_b.len(),
                        ob
                    );

                    let intent = Intent {
                        id: FihHash::new(&[topic, oa_sorted, ob_sorted], "cross-gap"),
                        from_facts,
                        description: desc,
                        creator: "gap-detector".into(),
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
