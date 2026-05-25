// nexus-process — Gap detector: identifies unexplored concept pairs.
//
// Detects orphaned Facts at two levels and records them as Facts:
//   1. Origin-based: Facts from the same origin with no grounding Intent.
//   2. Topic-based (cross-origin): Facts on the same topic from different
//      origins — a cross-document research gap.
//
// Implements: DetectionCapable + GapDetection (from nexus-model)
//
// Stigmergy principle: dumb observation, infinite iterations.
// This detector does not propose action (Intent) — it observes and records
// gaps as Facts. Other agents or later iterations may act on these Facts.

use super::common::topic_of;
use nexus_model::{BoardState, DetectionCapable, DetectionOutput, Fact, FihHash, GapDetection};
use std::collections::{HashMap, HashSet};

pub struct GapDetector {
    seen_origin: HashSet<(String, String)>,
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

impl GapDetection for GapDetector {}

impl DetectionCapable for GapDetector {
    fn name(&self) -> &str {
        "gap-detector"
    }

    fn orient(&mut self, state: &BoardState) -> DetectionOutput {
        // Only analyze document-source facts.
        // Detector output facts have origin "gap-detector" etc. —
        // they are observations, not primary sources to re-analyze.
        let doc_facts: Vec<&Fact> = state
            .facts
            .iter()
            .filter(|f| f.origin != "gap-detector")
            .collect();

        let referenced: HashSet<&str> = state
            .intents
            .iter()
            .flat_map(|i| i.from_facts.iter().map(|s| s.as_str()))
            .collect();

        let orphaned: Vec<&Fact> = doc_facts
            .iter()
            .filter(|f| !referenced.contains(f.id.0.as_str()))
            .copied()
            .collect();

        if orphaned.is_empty() {
            return DetectionOutput::default();
        }

        // Level 1: Origin-based grouping
        let mut by_origin: HashMap<&str, Vec<&Fact>> = HashMap::new();
        for f in &orphaned {
            by_origin.entry(f.origin.as_str()).or_default().push(f);
        }

        let mut output = DetectionOutput::default();

        for (origin, facts) in &by_origin {
            if facts.len() >= 2 {
                let mut ids: Vec<&str> = facts.iter().map(|f| f.id.0.as_str()).collect();
                ids.sort();
                let key = ((*origin).to_string(), ids.join(","));
                if self.seen_origin.contains(&key) {
                    continue;
                }
                self.seen_origin.insert(key.clone());

                output.facts.push(Fact {
                    id: FihHash::new(&[origin, "gap"], "fact"),
                    origin: "gap-detector".into(),
                    content: serde_json::json!({
                        "type": "gap",
                        "subtype": "origin-orphan",
                        "origin": origin,
                        "orphan_count": facts.len(),
                        "fact_ids": facts.iter().map(|f| &f.id.0).collect::<Vec<_>>(),
                    }),
                    creator: "gap-detector".into(),
                });
            }
        }

        // Level 2: Topic-based cross-origin grouping
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
                    let (oa_s, ob_s) = if oa < ob { (*oa, *ob) } else { (*ob, *oa) };
                    let key = (topic.to_string(), oa_s.to_string(), ob_s.to_string());
                    if self.seen_topic.contains(&key) {
                        continue;
                    }
                    self.seen_topic.insert(key.clone());

                    output.facts.push(Fact {
                        id: FihHash::new(&[topic, oa_s, ob_s], "cross-gap"),
                        origin: "gap-detector".into(),
                        content: serde_json::json!({
                            "type": "gap",
                            "subtype": "cross-origin",
                            "topic": topic,
                            "origin_a": oa_s,
                            "origin_b": ob_s,
                            "count_a": origins[oa].len(),
                            "count_b": origins[ob].len(),
                        }),
                        creator: "gap-detector".into(),
                    });
                }
            }
        }

        output
    }
}
