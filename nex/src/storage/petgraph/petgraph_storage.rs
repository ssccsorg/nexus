// nexus-graph — PetgraphStorage: in-memory HotStorage implementation.
//
// Wraps petgraph::Graph in Arc<RwLock<>> for thread-safe shared access on
// native targets, and Rc<RefCell<>> (no lock) on WASM targets where
// std::sync::RwLock is broken (hangs after write, recursive panic, CPU
// timeout).
// Implements StorageRead, FactCapable, HintCapable, IntentCapable,
// TimeRangeCapable, EvictCapable, FilterCapable, and CypherCapable.

use super::weight::{EdgeWeight, NodeWeight};
use cfg_if::cfg_if;
use nexus_model::{
    BlackboardError, BoardState, Content, DeltaSet, EvictCapable, Fact, FactCapable, FihHash,
    FilterCapable, Hint, HintCapable, HotStorage, Intent, IntentCapable, Now, StateFilter,
    StorageRead, TimeRangeCapable,
};
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use postcard;
use std::collections::HashMap;
use std::ops::Range;

cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        use std::cell::RefCell;
        use std::rc::Rc;
        use std::cell::{Ref, RefMut};

        /// WASM: single-threaded, no lock needed. Rc<RefCell<>> for shared
        /// interior mutability.
        pub type SharedGraph = Rc<RefCell<petgraph::Graph<NodeWeight, EdgeWeight>>>;

        pub fn read_graph(g: &SharedGraph) -> Ref<'_, petgraph::Graph<NodeWeight, EdgeWeight>> {
            g.borrow()
        }

        pub fn write_graph(g: &SharedGraph) -> RefMut<'_, petgraph::Graph<NodeWeight, EdgeWeight>> {
            g.borrow_mut()
        }

        struct WasmClock;
        impl nexus_model::Now for WasmClock {
            fn now_nanos(&self) -> u64 { 0 }
            fn now_secs(&self) -> u64 { 0 }
        }
    } else {
        use std::sync::{Arc, RwLock};
        use std::sync::RwLockReadGuard;
        use std::sync::RwLockWriteGuard;

        /// Native: thread-safe shared access via Arc<RwLock<>>.
        pub type SharedGraph = Arc<RwLock<petgraph::Graph<NodeWeight, EdgeWeight>>>;

        pub fn read_graph(g: &SharedGraph) -> RwLockReadGuard<'_, petgraph::Graph<NodeWeight, EdgeWeight>> {
            g.read().unwrap()
        }

        pub fn write_graph(g: &SharedGraph) -> RwLockWriteGuard<'_, petgraph::Graph<NodeWeight, EdgeWeight>> {
            g.write().unwrap()
        }
    }
}

/// In-memory petgraph-backed storage.
///
/// On native, the underlying `petgraph::Graph` is shared through an
/// `Arc<RwLock<...>>` so that `DefaultBlackboard` can access the same graph
/// for Cypher queries while `PetgraphStorage` handles FIH persistence.
/// On WASM, uses `Rc<RefCell<...>>` since WASM is single-threaded.
pub struct PetgraphStorage {
    pub graph: SharedGraph,
    pub project_id: String,
    clock: Box<dyn Now + Send>,
}

impl PetgraphStorage {
    pub fn new() -> Self {
        Self::with_project_id("default")
    }

    pub fn with_project_id(project_id: &str) -> Self {
        cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                Self {
                    graph: Rc::new(RefCell::new(petgraph::Graph::new())),
                    project_id: project_id.to_string(),
                    clock: Self::default_clock(),
                }
            } else {
                Self {
                    graph: Arc::new(RwLock::new(petgraph::Graph::new())),
                    project_id: project_id.to_string(),
                    clock: Self::default_clock(),
                }
            }
        }
    }

    pub fn with_shared_graph(graph: SharedGraph, project_id: &str) -> Self {
        Self {
            graph,
            project_id: project_id.to_string(),
            clock: Self::default_clock(),
        }
    }

    /// Create a PetgraphStorage wrapping a clone of the given graph data.
    /// Used by snapshot restoration to seed the hot graph from saved state.
    pub fn with_shared_graph_from_data(
        graph_data: petgraph::Graph<NodeWeight, EdgeWeight>,
        project_id: &str,
    ) -> Self {
        cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                Self {
                    graph: Rc::new(RefCell::new(graph_data)),
                    project_id: project_id.to_string(),
                    clock: Self::default_clock(),
                }
            } else {
                Self {
                    graph: Arc::new(RwLock::new(graph_data)),
                    project_id: project_id.to_string(),
                    clock: Self::default_clock(),
                }
            }
        }
    }

    fn default_clock() -> Box<dyn Now + Send> {
        cfg_if! {
            if #[cfg(target_arch = "wasm32")] {
                Box::new(WasmClock)
            } else {
                Box::new(nexus_model::SystemClock)
            }
        }
    }

    /// Replace the clock. Use this to inject FakeClock (testing) or
    /// HybridLogicalClock (distributed).
    pub fn with_clock(mut self, clock: Box<dyn Now + Send>) -> Self {
        self.clock = clock;
        self
    }

    pub fn with_graph<R>(
        &self,
        f: impl FnOnce(&petgraph::Graph<NodeWeight, EdgeWeight>) -> R,
    ) -> R {
        let g = read_graph(&self.graph);
        f(&g)
    }

    pub fn with_graph_mut<R>(
        &self,
        f: impl FnOnce(&mut petgraph::Graph<NodeWeight, EdgeWeight>) -> R,
    ) -> R {
        let mut g = write_graph(&self.graph);
        f(&mut g)
    }

    pub fn flush(&self) -> Result<(), String> {
        Ok(())
    }

    /// Read all nodes with submitted_at > cursor_timestamp.
    /// Returns (facts, intents, hints) as postcard-serialized blobs.
    pub fn read_delta_since(&self, cursor_ts: &str) -> DeltaSet {
        let since_ts: u128 = if cursor_ts.is_empty() {
            0
        } else {
            cursor_ts.parse().unwrap_or(0)
        };
        let g = read_graph(&self.graph);
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
                .and_then(|c| c.as_str())
                .unwrap_or("0");
            let ts: u128 = ts_str.parse().unwrap_or(0);
            if ts <= since_ts {
                continue;
            }
            match w.label.as_str() {
                "Fact" => {
                    if let Ok(line) = postcard::to_allocvec(&Fact {
                        id: FihHash(w.name.clone()),
                        origin: w
                            .properties
                            .get("origin")
                            .and_then(|c| c.as_str())
                            .unwrap_or("")
                            .into(),
                        content: w
                            .properties
                            .get("content")
                            .and_then(|c| c.as_str())
                            .map(|s| Content {
                                mime_type: "text/plain".into(),
                                data: s.as_bytes().to_vec(),
                            })
                            .unwrap_or_else(|| Content {
                                mime_type: "application/octet-stream".into(),
                                data: Vec::new(),
                            }),
                        creator: w
                            .properties
                            .get("creator")
                            .and_then(|c| c.as_str())
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
                    if let Ok(line) = postcard::to_allocvec(&Intent {
                        id: FihHash(w.name.clone()),
                        from_facts,
                        description: w
                            .properties
                            .get("description")
                            .and_then(|c| c.as_str())
                            .unwrap_or("")
                            .into(),
                        creator: w
                            .properties
                            .get("creator")
                            .and_then(|c| c.as_str())
                            .unwrap_or("")
                            .into(),
                        worker: w
                            .properties
                            .get("worker")
                            .and_then(|c| c.as_str())
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
                            .and_then(|c| c.as_str())
                            .and_then(|s| s.parse::<u64>().ok()),
                        created_at: w
                            .properties
                            .get("created_at")
                            .and_then(|c| c.as_str())
                            .and_then(|s| s.parse::<u64>().ok()),
                        concluded: false,
                        concluded_at: None,
                    }) {
                        intents.push(line);
                    }
                }
                "Hint" => {
                    if let Ok(line) = postcard::to_allocvec(&Hint {
                        id: FihHash(w.name.clone()),
                        content: w
                            .properties
                            .get("content")
                            .and_then(|c| c.as_str())
                            .unwrap_or("")
                            .into(),
                        creator: w
                            .properties
                            .get("creator")
                            .and_then(|c| c.as_str())
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
        let g = read_graph(&self.graph);
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
                                .and_then(|c| c.as_str())
                                .unwrap_or("")
                                .into(),
                            content: w
                                .properties
                                .get("content")
                                .and_then(|c| c.as_str())
                                .map(|s| Content {
                                    mime_type: "text/plain".into(),
                                    data: s.as_bytes().to_vec(),
                                })
                                .unwrap_or_else(|| Content {
                                    mime_type: "application/octet-stream".into(),
                                    data: Vec::new(),
                                }),
                            creator: w
                                .properties
                                .get("creator")
                                .and_then(|c| c.as_str())
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
                                .and_then(|c| c.as_str())
                                .unwrap_or("")
                                .into(),
                            creator: w
                                .properties
                                .get("creator")
                                .and_then(|c| c.as_str())
                                .unwrap_or("")
                                .into(),
                            worker: w
                                .properties
                                .get("worker")
                                .and_then(|c| c.as_str())
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
                                .and_then(|c| c.as_str())
                                .and_then(|s| s.parse::<u64>().ok()),
                            created_at: w
                                .properties
                                .get("created_at")
                                .and_then(|c| c.as_str())
                                .and_then(|s| s.parse::<u64>().ok()),
                            concluded: w
                                .properties
                                .get("concluded")
                                .and_then(|c| c.as_str())
                                .is_some_and(|v| v == "true"),
                            concluded_at: if w
                                .properties
                                .get("concluded")
                                .and_then(|c| c.as_str())
                                .is_some_and(|v| v == "true")
                            {
                                Some(1)
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
                                .and_then(|c| c.as_str())
                                .unwrap_or("")
                                .into(),
                            creator: w
                                .properties
                                .get("creator")
                                .and_then(|c| c.as_str())
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
        let mut g = write_graph(&self.graph);
        let now = self.clock.now_nanos().to_string();
        let content_val = String::from_utf8_lossy(&fact.content.data).into_owned();
        g.add_node(NodeWeight {
            name: fact.id.0.clone(),
            label: "Fact".into(),
            properties: {
                let mut m = HashMap::new();
                m.insert(
                    "origin".into(),
                    Content {
                        mime_type: "text/plain".into(),
                        data: fact.origin.clone().into_bytes(),
                    },
                );
                m.insert(
                    "content".into(),
                    Content {
                        mime_type: "text/plain".into(),
                        data: content_val.into_bytes(),
                    },
                );
                m.insert(
                    "creator".into(),
                    Content {
                        mime_type: "text/plain".into(),
                        data: fact.creator.clone().into_bytes(),
                    },
                );
                m.insert(
                    "submitted_at".into(),
                    Content {
                        mime_type: "text/plain".into(),
                        data: now.into_bytes(),
                    },
                );
                m
            },
        });
        Ok(fact.id.clone())
    }
}

impl HintCapable for PetgraphStorage {
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        let mut g = write_graph(&self.graph);
        g.add_node(NodeWeight {
            name: hint.id.0.clone(),
            label: "Hint".into(),
            properties: {
                let mut m = HashMap::new();
                m.insert(
                    "content".into(),
                    Content {
                        mime_type: "text/plain".into(),
                        data: hint.content.clone().into_bytes(),
                    },
                );
                m.insert(
                    "creator".into(),
                    Content {
                        mime_type: "text/plain".into(),
                        data: hint.creator.clone().into_bytes(),
                    },
                );
                let now = self.clock.now_nanos().to_string();
                m.insert(
                    "submitted_at".into(),
                    Content {
                        mime_type: "text/plain".into(),
                        data: now.into_bytes(),
                    },
                );
                m
            },
        });
        Ok(())
    }
}

impl IntentCapable for PetgraphStorage {
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        let mut g = write_graph(&self.graph);

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
                m.insert(
                    "description".into(),
                    Content {
                        mime_type: "text/plain".into(),
                        data: intent.description.clone().into_bytes(),
                    },
                );
                m.insert(
                    "creator".into(),
                    Content {
                        mime_type: "text/plain".into(),
                        data: intent.creator.clone().into_bytes(),
                    },
                );
                let now = self.clock.now_secs().to_string();
                m.insert(
                    "created_at".into(),
                    Content {
                        mime_type: "text/plain".into(),
                        data: now.into_bytes(),
                    },
                );
                let now_ns = self.clock.now_nanos().to_string();
                m.insert(
                    "submitted_at".into(),
                    Content {
                        mime_type: "text/plain".into(),
                        data: now_ns.into_bytes(),
                    },
                );
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
        let mut g = write_graph(&self.graph);
        for idx in g.node_indices() {
            if let Some(w) = g.node_weight_mut(idx)
                && w.name == intent_id
                && w.label == "Intent"
            {
                if w.properties
                    .get("concluded")
                    .is_some_and(|v| v.as_str() == Some("true"))
                {
                    return Err(BlackboardError::NotFound(format!(
                        "Intent {intent_id} already concluded"
                    )));
                }
                if let Some(current) = w.properties.get("worker") {
                    return Err(BlackboardError::Conflict(format!(
                        "Intent {intent_id} already claimed by {}",
                        current.as_str().unwrap_or("?")
                    )));
                }
                w.properties.insert(
                    "worker".into(),
                    Content {
                        mime_type: "text/plain".into(),
                        data: agent.to_string().into_bytes(),
                    },
                );
                let now_secs = self.clock.now_secs();
                w.properties.insert(
                    "last_heartbeat_at".into(),
                    Content {
                        mime_type: "text/plain".into(),
                        data: now_secs.to_string().into_bytes(),
                    },
                );
                return Ok(());
            }
        }
        Err(BlackboardError::NotFound(format!(
            "Intent {intent_id} not found"
        )))
    }

    fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        let mut g = write_graph(&self.graph);
        for idx in g.node_indices() {
            if let Some(w) = g.node_weight_mut(idx)
                && w.name == intent_id
                && w.label == "Intent"
            {
                if w.properties
                    .get("concluded")
                    .is_some_and(|v| v.as_str() == Some("true"))
                {
                    return Err(BlackboardError::NotFound(format!(
                        "Intent {intent_id} already concluded"
                    )));
                }
                let current = w.properties.get("worker").and_then(|c| c.as_str());
                match current {
                    Some(existing) if existing != agent => {
                        return Err(BlackboardError::Conflict(format!(
                            "Intent {intent_id} is claimed by {existing}, not {agent}"
                        )));
                    }
                    _ => {
                        w.properties.insert(
                            "worker".into(),
                            Content {
                                mime_type: "text/plain".into(),
                                data: agent.to_string().into_bytes(),
                            },
                        );
                        let now_secs = self.clock.now_secs();
                        w.properties.insert(
                            "last_heartbeat_at".into(),
                            Content {
                                mime_type: "text/plain".into(),
                                data: now_secs.to_string().into_bytes(),
                            },
                        );
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
        let mut g = write_graph(&self.graph);
        for idx in g.node_indices() {
            if let Some(w) = g.node_weight_mut(idx)
                && w.name == intent_id
                && w.label == "Intent"
            {
                if w.properties
                    .get("concluded")
                    .is_some_and(|v| v.as_str() == Some("true"))
                {
                    return Err(BlackboardError::NotFound(format!(
                        "Intent {intent_id} already concluded"
                    )));
                }
                let current = w
                    .properties
                    .get("worker")
                    .and_then(|c| c.as_str())
                    .map(|s| s.to_string());
                match current {
                    None => return Ok(()),
                    Some(existing) if existing != agent => {
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

    fn conclude_intent(&self, intent_id: &str, result: &str) -> Result<Fact, BlackboardError> {
        let mut g = write_graph(&self.graph);

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
            .and_then(|c| c.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        if let Some(w) = g.node_weight_mut(intent_idx) {
            w.properties.insert(
                "concluded".into(),
                Content {
                    mime_type: "text/plain".into(),
                    data: b"true".to_vec(),
                },
            );
            w.properties.remove("worker");
        }

        let content_val = result.to_string();
        let content_for_fact = Content {
            mime_type: "text/plain".into(),
            data: result.to_string().into_bytes(),
        };
        let new_fact_id = format!("f_concl_{}", intent_id);
        let new_fact = Fact {
            id: FihHash(new_fact_id.clone()),
            origin: format!("conclusion:{}", intent_id),
            content: content_for_fact,
            creator: worker,
        };

        let fact_idx = g.add_node(NodeWeight {
            name: new_fact_id,
            label: "Fact".into(),
            properties: {
                let mut m = HashMap::new();
                m.insert(
                    "origin".into(),
                    Content {
                        mime_type: "text/plain".into(),
                        data: new_fact.origin.clone().into_bytes(),
                    },
                );
                m.insert(
                    "content".into(),
                    Content {
                        mime_type: "text/plain".into(),
                        data: content_val.into_bytes(),
                    },
                );
                m.insert(
                    "creator".into(),
                    Content {
                        mime_type: "text/plain".into(),
                        data: new_fact.creator.clone().into_bytes(),
                    },
                );
                let now_ns = self.clock.now_nanos().to_string();
                m.insert(
                    "submitted_at".into(),
                    Content {
                        mime_type: "text/plain".into(),
                        data: now_ns.into_bytes(),
                    },
                );
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
        let g = read_graph(&self.graph);
        let mut total = 0usize;
        for idx in g.node_indices() {
            if let Some(w) = g.node_weight(idx) {
                total += w.name.len() + w.label.len() + 64;
                for (k, v) in &w.properties {
                    total += k.len();
                    total += v.data.len();
                }
            }
        }
        for idx in g.edge_indices() {
            if let Some(e) = g.edge_weight(idx) {
                total += e.rel_type.len() + 32;
                for (k, v) in &e.properties {
                    total += k.len();
                    total += v.data.len();
                }
            }
        }
        total
    }

    fn evict_before(&self, before: &str) -> Result<u64, String> {
        let before_secs: u64 = before
            .parse()
            .map_err(|e| format!("invalid timestamp: {e}"))?;
        let mut g = write_graph(&self.graph);

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
                    .and_then(|c| c.as_str())
                    .is_some_and(|v| v == "true");
                let hb_ts = w
                    .properties
                    .get("last_heartbeat_at")
                    .and_then(|c| c.as_str())
                    .and_then(|s| s.parse::<i64>().ok());

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
        let now = self.clock.now_secs();
        let cutoff = now.saturating_sub(older_than_secs);

        let mut g = write_graph(&self.graph);
        let mut to_remove: Vec<NodeIndex> = Vec::new();

        for idx in g.node_indices() {
            let Some(w) = g.node_weight(idx) else {
                continue;
            };
            if w.label.as_str() != "Intent" {
                continue;
            }
            let is_concluded = w
                .properties
                .get("concluded")
                .and_then(|c| c.as_str())
                .is_some_and(|v| v == "true");
            if is_concluded {
                continue;
            }
            let has_worker = w.properties.contains_key("worker");
            if has_worker {
                continue;
            }
            let created = w
                .properties
                .get("created_at")
                .and_then(|c| c.as_str())
                .and_then(|s| s.parse::<i64>().ok())
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
    fn read_delta_since(&self, cursor_ts: &str) -> (Vec<Vec<u8>>, Vec<Vec<u8>>, Vec<Vec<u8>>) {
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
                    .map(|c| c as u128)
                    .is_none_or(|ts| ts >= since_ts)
            });
        }
        if let Some(until_str) = &filter.until
            && let Ok(until_ts) = until_str.parse::<u128>()
        {
            state.intents.retain(|i| {
                i.created_at
                    .map(|c| c as u128)
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
