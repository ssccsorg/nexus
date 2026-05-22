// nexus-process — Gap detector: identifies unexplored concept pairs.
//
// A simple stigmergy task: when two Facts with different origins discuss
// related concepts, submit an Intent to explore the intersection.
// This is the "many iterations" heuristic — eventually, every pair gets
// an Intent if it's interesting enough.

use super::{TaskHandler, TaskOutput};
use nexus_model::{BoardState, Fact, FihHash, Intent};
use std::collections::HashSet;

/// A gap detector that spots orphaned concepts (Facts with no Intent
/// grounding them to other Facts).
///
/// Tracks previously-synthesised (origin, fact_count) pairs to avoid
/// submitting duplicate Intents on successive OODA ticks.
pub struct GapDetector {
    /// Set of (origin, fact_count) tuples already synthesised.
    seen: HashSet<(String, usize)>,
}

impl GapDetector {
    pub fn new() -> Self {
        Self {
            seen: HashSet::new(),
        }
    }
}

impl Default for GapDetector {
    fn default() -> Self {
        Self::new()
    }
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

        // Group orphaned facts by origin
        let mut by_origin: std::collections::HashMap<&str, Vec<&Fact>> =
            std::collections::HashMap::new();
        for f in &orphaned {
            by_origin.entry(f.origin.as_str()).or_default().push(f);
        }

        let mut output = TaskOutput::default();

        // For origins with multiple orphaned facts, submit a synthesis Intent
        for (origin, facts) in &by_origin {
            if facts.len() >= 2 {
                let key = ((*origin).to_string(), facts.len());
                if self.seen.contains(&key) {
                    continue; // already synthesised this exact set
                }
                self.seen.insert(key);

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

        output
    }
}
