// nexus-graph — PetgraphStorage: in-memory HotStorage implementation.
//
// Wraps petgraph::Graph in Arc<RwLock<>> for thread-safe shared access.
// Implements StorageRead, FactCapable, HintCapable, IntentCapable,
// TimeRangeCapable, EvictCapable, FilterCapable, and CypherCapable.

use crate::weight::{EdgeWeight, NodeWeight};
use nexus_model::{
    BlackboardError, BoardState, CypherCapable, EvictCapable, Fact, FactCapable, FihHash,
    FilterCapable, Hint, HintCapable, HotStorage, Intent, IntentCapable, StateFilter, StorageRead,
    TimeRangeCapable,
};
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, RwLock};

/// In-memory petgraph-backed storage. Thread-safe via internal RwLock.
///
/// The underlying `petgraph::Graph` is shared through an `Arc<RwLock<...>>`
/// so that `DefaultBlackboard` can access the same graph for Cypher queries
/// while `PetgraphStorage` handles FIH persistence.
pub struct PetgraphStorage {
    pub graph: Arc<RwLock<petgraph::Graph<NodeWeight, EdgeWeight>>>,
    pub project_id: String,
}

impl PetgraphStorage {
    pub fn new() -> Self {
        Self::with_project_id("default")
    }

    pub fn with_project_id(project_id: &str) -> Self {
        Self {
            graph: Arc::new(RwLock::new(petgraph::Graph::new())),
            project_id: project_id.to_string(),
        }
    }

    pub fn with_shared_graph(
        graph: Arc<RwLock<petgraph::Graph<NodeWeight, EdgeWeight>>>,
        project_id: &str,
    ) -> Self {
        Self {
            graph,
            project_id: project_id.to_string(),
        }
    }

    pub fn with_graph<R>(
        &self,
        f: impl FnOnce(&petgraph::Graph<NodeWeight, EdgeWeight>) -> R,
    ) -> R {
        let g = self.graph.read().unwrap();
        f(&g)
    }

    pub fn with_graph_mut<R>(
        &self,
        f: impl FnOnce(&mut petgraph::Graph<NodeWeight, EdgeWeight>) -> R,
    ) -> R {
        let mut g = self.graph.write().unwrap();
        f(&mut g)
    }

    pub fn flush(&self) -> Result<(), String> {
        Ok(())
    }

    /// Read all nodes with submitted_at > cursor_timestamp.
    /// Returns (facts, intents, hints) as JSON-lines strings.
    pub fn read_delta_since(&self, cursor_ts: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
        let since_ts: u128 = if cursor_ts.is_empty() {
            0
        } else {
            cursor_ts.parse().unwrap_or(0)
        };
        let g = self.graph.read().unwrap();
        let mut facts = Vec::new();
        let mut intents = Vec::new();
        let mut hints = Vec::new();

        for idx in g.node_indices() {
            let Some(w) = g.node_weight(idx) else {
                continue;
            };
            let ts_str = w
                .properties
                .get("submitted_at")
                .and_then(|v| v.as_str())
                .unwrap_or("0");
            let ts: u128 = ts_str.parse().unwrap_or(0);
            if ts <= since_ts {
                continue;
            }
            match w.label.as_str() {
                "Fact" => {
                    if let Ok(line) = serde_json::to_string(&Fact {
                        id: FihHash(w.name.clone()),
                        origin: w
                            .properties
                            .get("origin")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .into(),
                        content: w
                            .properties
                            .get("content")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null),
                        creator: w
                            .properties
                            .get("creator")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .into(),
                    }) {
                        facts.push(line);
                    }
                }
                "Intent" => {
                    let from_facts: Vec<String> = g
                        .edges_directed(idx, petgraph::Direction::Incoming)
                        .filter_map(|e| {
                            let sn = g.node_weight(e.source())?;
                            Some(sn.name.clone())
                        })
                        .collect();
                    if let Ok(line) = serde_json::to_string(&Intent {
                        id: FihHash(w.name.clone()),
                        from_facts,
                        description: w
                            .properties
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .into(),
                        creator: w
                            .properties
                            .get("creator")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .into(),
                        worker: w
                            .properties
                            .get("worker")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        to_fact_id: {
                            g.edges_directed(idx, petgraph::Direction::Outgoing)
                                .find(|e| {
                                    g.node_weight(e.target()).is_some_and(|n| {
                                        n.label == "Fact" && n.name.starts_with("f_concl_")
                                    })
                                })
                                .and_then(|e| g.node_weight(e.target()).map(|n| n.name.clone()))
                        },
                        last_heartbeat_at: w
                            .properties
                            .get("last_heartbeat_at")
                            .and_then(|v| v.as_i64())
                            .map(|ts| ts.to_string()),
                        created_at: w
                            .properties
                            .get("created_at")
                            .and_then(|v| v.as_i64())
                            .map(|ts| ts.to_string()),
                        concluded_at: None,
                    }) {
                        intents.push(line);
                    }
                }
                "Hint" => {
                    if let Ok(line) = serde_json::to_string(&Hint {
                        id: FihHash(w.name.clone()),
                        content: w
                            .properties
                            .get("content")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .into(),
                        creator: w
                            .properties
                            .get("creator")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .into(),
                    }) {
                        hints.push(line);
                    }
                }
                _ => {}
            }
        }
        (facts, intents, hints)
    }
}

impl Default for PetgraphStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageRead for PetgraphStorage {
    fn project_id(&self) -> &str {
        &self.project_id
    }

    fn read_state(&self) -> BoardState {
        let g = self.graph.read().unwrap();
        let mut facts = Vec::new();
        let mut intents = Vec::new();
        let mut hints = Vec::new();

        for idx in g.node_indices() {
            if let Some(w) = g.node_weight(idx) {
                match w.label.as_str() {
                    "Fact" => {
                        facts.push(Fact {
                            id: FihHash(w.name.clone()),
                            origin: w
                                .properties
                                .get("origin")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .into(),
                            content: w
                                .properties
                                .get("content")
                                .cloned()
                                .unwrap_or(serde_json::Value::Null),
                            creator: w
                                .properties
                                .get("creator")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .into(),
                        });
                    }
                    "Intent" => {
                        let from_facts: Vec<String> = g
                            .edges_directed(idx, petgraph::Direction::Incoming)
                            .filter_map(|e| {
                                let sn = g.node_weight(e.source())?;
                                Some(sn.name.clone())
                            })
                            .collect();

                        intents.push(Intent {
                            id: FihHash(w.name.clone()),
                            from_facts,
                            description: w
                                .properties
                                .get("description")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .into(),
                            creator: w
                                .properties
                                .get("creator")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .into(),
                            worker: w
                                .properties
                                .get("worker")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                            to_fact_id: {
                                g.edges_directed(idx, petgraph::Direction::Outgoing)
                                    .find(|e| {
                                        g.node_weight(e.target()).is_some_and(|n| {
                                            n.label == "Fact" && n.name.starts_with("f_concl_")
                                        })
                                    })
                                    .and_then(|e| g.node_weight(e.target()).map(|n| n.name.clone()))
                            },
                            last_heartbeat_at: w
                                .properties
                                .get("last_heartbeat_at")
                                .and_then(|v| v.as_i64())
                                .map(|ts| ts.to_string()),
                            created_at: w
                                .properties
                                .get("created_at")
                                .and_then(|v| v.as_i64())
                                .map(|ts| ts.to_string()),
                            concluded_at: if w
                                .properties
                                .get("concluded")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false)
                            {
                                Some("yes".into())
                            } else {
                                None
                            },
                        });
                    }
                    "Hint" => {
                        hints.push(Hint {
                            id: FihHash(w.name.clone()),
                            content: w
                                .properties
                                .get("content")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .into(),
                            creator: w
                                .properties
                                .get("creator")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .into(),
                        });
                    }
                    _ => {}
                }
            }
        }

        BoardState {
            facts,
            intents,
            hints,
        }
    }
}

impl FactCapable for PetgraphStorage {
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        let mut g = self.graph.write().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_string();
        g.add_node(NodeWeight {
            name: fact.id.0.clone(),
            label: "Fact".into(),
            properties: {
                let mut m = HashMap::new();
                m.insert("origin".into(), fact.origin.clone().into());
                m.insert("content".into(), fact.content.clone());
                m.insert("creator".into(), fact.creator.clone().into());
                m.insert("submitted_at".into(), now.into());
                m
            },
        });
        Ok(fact.id.clone())
    }
}

impl HintCapable for PetgraphStorage {
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        let mut g = self.graph.write().unwrap();
        g.add_node(NodeWeight {
            name: hint.id.0.clone(),
            label: "Hint".into(),
            properties: {
                let mut m = HashMap::new();
                m.insert("content".into(), hint.content.clone().into());
                m.insert("creator".into(), hint.creator.clone().into());
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
                    .to_string();
                m.insert("submitted_at".into(), now.into());
                m
            },
        });
        Ok(())
    }
}

impl IntentCapable for PetgraphStorage {
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        let mut g = self.graph.write().unwrap();

        for fid in &intent.from_facts {
            let found = g
                .node_indices()
                .any(|i| g.node_weight(i).is_some_and(|w| w.name == *fid));
            if !found {
                return Err(BlackboardError::NotFound(format!("Fact {fid} not found")));
            }
        }

        let idx = g.add_node(NodeWeight {
            name: intent.id.0.clone(),
            label: "Intent".into(),
            properties: {
                let mut m = HashMap::new();
                m.insert("description".into(), intent.description.clone().into());
                m.insert("creator".into(), intent.creator.clone().into());
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                m.insert("created_at".into(), serde_json::json!(now));
                let now_ns = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
                    .to_string();
                m.insert("submitted_at".into(), now_ns.into());
                m
            },
        });

        for fid in &intent.from_facts {
            if let Some(src) = g
                .node_indices()
                .find(|i| g.node_weight(*i).is_some_and(|w| w.name == *fid))
            {
                g.add_edge(
                    src,
                    idx,
                    EdgeWeight {
                        rel_type: "drives".into(),
                        properties: HashMap::new(),
                    },
                );
            }
        }

        Ok(intent.id.clone())
    }

    fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let mut g = self.graph.write().unwrap();
        for idx in g.node_indices() {
            if let Some(w) = g.node_weight_mut(idx)
                && w.name == intent_id
                && w.label == "Intent"
            {
                if w.properties
                    .get("concluded")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    return Err(BlackboardError::NotFound(format!(
                        "Intent {intent_id} already concluded"
                    )));
                }
                if let Some(serde_json::Value::String(current)) = w.properties.get("worker") {
                    return Err(BlackboardError::Conflict(format!(
                        "Intent {intent_id} already claimed by {current}"
                    )));
                }
                w.properties
                    .insert("worker".into(), agent.to_string().into());
                if let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                {
                    w.properties.insert(
                        "last_heartbeat_at".into(),
                        serde_json::Value::Number(now.as_secs().into()),
                    );
                }
                return Ok(());
            }
        }
        Err(BlackboardError::NotFound(format!(
            "Intent {intent_id} not found"
        )))
    }

    fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let mut g = self.graph.write().unwrap();
        for idx in g.node_indices() {
            if let Some(w) = g.node_weight_mut(idx)
                && w.name == intent_id
                && w.label == "Intent"
            {
                if w.properties
                    .get("concluded")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    return Err(BlackboardError::NotFound(format!(
                        "Intent {intent_id} already concluded"
                    )));
                }
                let current = w.properties.get("worker");
                match current {
                    Some(serde_json::Value::String(existing)) if existing != agent => {
                        return Err(BlackboardError::Conflict(format!(
                            "Intent {intent_id} is claimed by {existing}, not {agent}"
                        )));
                    }
                    _ => {
                        w.properties
                            .insert("worker".into(), agent.to_string().into());
                        // Record heartbeat timestamp for TTL monitoring
                        if let Ok(now) =
                            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                        {
                            w.properties.insert(
                                "last_heartbeat_at".into(),
                                serde_json::Value::Number(now.as_secs().into()),
                            );
                        }
                        return Ok(());
                    }
                }
            }
        }
        Err(BlackboardError::NotFound(format!(
            "Intent {intent_id} not found"
        )))
    }

    fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let mut g = self.graph.write().unwrap();
        for idx in g.node_indices() {
            if let Some(w) = g.node_weight_mut(idx)
                && w.name == intent_id
                && w.label == "Intent"
            {
                if w.properties
                    .get("concluded")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    return Err(BlackboardError::NotFound(format!(
                        "Intent {intent_id} already concluded"
                    )));
                }
                let current = w.properties.get("worker").cloned();
                match current {
                    None => return Ok(()),
                    Some(serde_json::Value::String(existing)) if existing != agent => {
                        return Err(BlackboardError::Forbidden(format!(
                            "Intent {intent_id} claimed by {existing}"
                        )));
                    }
                    _ => {
                        w.properties.remove("worker");
                        return Ok(());
                    }
                }
            }
        }
        Err(BlackboardError::NotFound(format!(
            "Intent {intent_id} not found"
        )))
    }

    fn conclude_intent(
        &self,
        intent_id: &str,
        result: &serde_json::Value,
    ) -> Result<Fact, BlackboardError> {
        let mut g = self.graph.write().unwrap();

        let intent_idx = g
            .node_indices()
            .find(|i| {
                g.node_weight(*i)
                    .is_some_and(|w| w.name == intent_id && w.label == "Intent")
            })
            .ok_or_else(|| BlackboardError::NotFound(format!("Intent {intent_id} not found")))?;

        let worker = g
            .node_weight(intent_idx)
            .and_then(|w| w.properties.get("worker"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        if let Some(w) = g.node_weight_mut(intent_idx) {
            w.properties
                .insert("concluded".into(), serde_json::Value::Bool(true));
            w.properties.remove("worker");
        }

        let new_fact_id = format!("f_concl_{}", intent_id);
        let new_fact = Fact {
            id: FihHash(new_fact_id.clone()),
            origin: format!("conclusion:{}", intent_id),
            content: result.clone(),
            creator: worker,
        };

        let fact_idx = g.add_node(NodeWeight {
            name: new_fact_id,
            label: "Fact".into(),
            properties: {
                let mut m = HashMap::new();
                m.insert("origin".into(), new_fact.origin.clone().into());
                m.insert("content".into(), new_fact.content.clone());
                m.insert("creator".into(), new_fact.creator.clone().into());
                let now_ns = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
                    .to_string();
                m.insert("submitted_at".into(), now_ns.into());
                m
            },
        });

        g.add_edge(
            intent_idx,
            fact_idx,
            EdgeWeight {
                rel_type: "concludes".into(),
                properties: HashMap::new(),
            },
        );

        Ok(new_fact)
    }
}

impl TimeRangeCapable for PetgraphStorage {
    fn time_range(&self) -> Option<Range<String>> {
        // petgraph is an in-memory hot store with no inherent time bound.
        // Returns None (universal range) since it can hold any data.
        None
    }
}

impl EvictCapable for PetgraphStorage {
    fn approximate_size(&self) -> usize {
        let g = self.graph.read().unwrap();
        let mut total = 0usize;
        for idx in g.node_indices() {
            if let Some(w) = g.node_weight(idx) {
                // Base overhead: name + label + HashMap
                total += w.name.len() + w.label.len() + 64;
                for (k, v) in &w.properties {
                    total += k.len();
                    match v {
                        serde_json::Value::String(s) => total += s.len(),
                        serde_json::Value::Number(n) => total += n.to_string().len(),
                        serde_json::Value::Array(arr) => total += arr.len() * 16,
                        serde_json::Value::Object(obj) => total += obj.len() * 32,
                        _ => total += 8,
                    }
                }
            }
        }
        // Account for edges: each edge stores rel_type + properties
        for idx in g.edge_indices() {
            if let Some(e) = g.edge_weight(idx) {
                total += e.rel_type.len() + 32;
                for (k, v) in &e.properties {
                    total += k.len() + 8;
                    if let serde_json::Value::String(s) = v {
                        total += s.len();
                    }
                }
            }
        }
        total
    }

    fn evict_before(&self, before: &str) -> Result<u64, String> {
        let before_secs: u64 = before
            .parse()
            .map_err(|e| format!("invalid timestamp: {e}"))?;
        let mut g = self.graph.write().unwrap();

        // Phase 1: collect intent nodes that are either:
        //   - concluded and older than `before`, OR
        //   - claimed but heartbeat expired before `before`
        let mut to_remove: Vec<NodeIndex> = Vec::new();
        let mut referenced_fact_names: std::collections::HashSet<String> =
            std::collections::HashSet::new();

        for idx in g.node_indices() {
            let Some(w) = g.node_weight(idx) else {
                continue;
            };
            if w.label.as_str() == "Intent" {
                let is_concluded = w
                    .properties
                    .get("concluded")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let hb_ts = w
                    .properties
                    .get("last_heartbeat_at")
                    .and_then(|v| v.as_i64());

                let should_evict = match (is_concluded, hb_ts) {
                    (true, Some(ts)) => (ts as u64) < before_secs,
                    (true, None) => true,
                    (false, Some(ts)) => (ts as u64) < before_secs,
                    (false, None) => false,
                };

                if should_evict {
                    to_remove.push(idx);
                } else {
                    for e in g.edges_directed(idx, petgraph::Direction::Incoming) {
                        if let Some(sn) = g.node_weight(e.source()) {
                            referenced_fact_names.insert(sn.name.clone());
                        }
                    }
                }
            }
        }

        // Phase 2: (removed) Facts are NEVER removed by eviction.
        // A Fact is an immutable observation. It must persist indefinitely.
        // Memory pressure should be managed by flush-to-cold, not Fact deletion.

        // Phase 3: remove collected Intent nodes only
        let removed = to_remove.len() as u64;
        for idx in to_remove {
            g.remove_node(idx);
        }

        Ok(removed)
    }

    fn evict_stale_intents(&self, older_than_secs: u64) -> Result<u64, String> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let cutoff = now.saturating_sub(older_than_secs);

        let mut g = self.graph.write().unwrap();
        let mut to_remove: Vec<NodeIndex> = Vec::new();

        for idx in g.node_indices() {
            let Some(w) = g.node_weight(idx) else {
                continue;
            };
            if w.label.as_str() != "Intent" {
                continue;
            }
            // Skip concluded intents — those are handled by evict_before
            let is_concluded = w
                .properties
                .get("concluded")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if is_concluded {
                continue;
            }
            // Skip claimed intents (have a worker)
            let has_worker = w.properties.contains_key("worker");
            if has_worker {
                continue;
            }
            // Check age
            let created = w
                .properties
                .get("created_at")
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as u64;
            if created < cutoff {
                to_remove.push(idx);
            }
        }

        let removed = to_remove.len() as u64;
        for idx in to_remove {
            g.remove_node(idx);
        }
        Ok(removed)
    }
}

impl HotStorage for PetgraphStorage {
    fn read_delta_since(&self, cursor_ts: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
        self.read_delta_since(cursor_ts)
    }
}

impl FilterCapable for PetgraphStorage {
    fn read_state_filtered(&self, filter: &StateFilter) -> BoardState {
        let mut state = self.read_state();

        if let Some(ids) = &filter.fact_ids {
            state.facts.retain(|f| ids.contains(&f.id.0));
        }
        if let Some(ids) = &filter.intent_ids {
            state.intents.retain(|i| ids.contains(&i.id.0));
        }
        if let Some(ids) = &filter.hint_ids {
            state.hints.retain(|h| ids.contains(&h.id.0));
        }

        if let Some(since_str) = &filter.since
            && let Ok(since_ts) = since_str.parse::<u128>()
        {
            state.intents.retain(|i| {
                i.created_at
                    .as_ref()
                    .and_then(|c| c.parse::<u128>().ok())
                    .is_none_or(|ts| ts >= since_ts)
            });
        }
        if let Some(until_str) = &filter.until
            && let Ok(until_ts) = until_str.parse::<u128>()
        {
            state.intents.retain(|i| {
                i.created_at
                    .as_ref()
                    .and_then(|c| c.parse::<u128>().ok())
                    .is_none_or(|ts| ts <= until_ts)
            });
        }

        let offset = filter.offset.unwrap_or(0);
        if let Some(limit) = filter.limit {
            state.facts = state.facts.into_iter().skip(offset).take(limit).collect();
            state.intents = state.intents.into_iter().skip(offset).take(limit).collect();
            state.hints = state.hints.into_iter().skip(offset).take(limit).collect();
        } else if offset > 0 {
            state.facts = state.facts.into_iter().skip(offset).collect();
            state.intents = state.intents.into_iter().skip(offset).collect();
            state.hints = state.hints.into_iter().skip(offset).collect();
        }

        state
    }
}

// ── CypherCapable ────────────────────────────────────────────────────────────

impl CypherCapable for PetgraphStorage {}
