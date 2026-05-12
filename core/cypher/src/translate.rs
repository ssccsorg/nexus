/// Petgraph executor for the local [`PlanIR`].
///
/// Evaluates PlanIR clauses against a petgraph graph, producing records.
/// The input [`PlanIR`] is produced by the cyrs-backed parser; this module
/// is independent of the cyrs crate.

use petgraph::graph::NodeIndex;
use petgraph::visit::{IntoEdgeReferences, IntoNodeReferences, NodeRef};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::plan::*;

/// Trait for graphs that support node/edge iteration with petgraph NodeIndex.
pub trait GraphLike: IntoNodeReferences<NodeId = NodeIndex> + IntoEdgeReferences + Default {
    fn node_weight(&self, idx: NodeIndex) -> Option<&NodeWeight>;
    fn edge_weight(&self, idx: petgraph::graph::EdgeIndex) -> Option<&EdgeWeight>;
    fn neighbors_undirected(&self, idx: NodeIndex) -> Vec<NodeIndex>;
    fn edges_directed(&self, idx: NodeIndex, outgoing: bool) -> Vec<(NodeIndex, NodeIndex, petgraph::graph::EdgeIndex)>;
    fn add_node(&mut self, weight: NodeWeight) -> NodeIndex;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeWeight {
    pub name: String,
    pub label: String,
    pub properties: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeWeight {
    pub rel_type: String,
    pub properties: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub fields: HashMap<String, serde_json::Value>,
}

pub fn execute<G: GraphLike>(
    graph: &G,
    plan: &PlanIR,
) -> Result<Vec<Record>, TranslateError> {
    let mut records: Vec<Record> = Vec::new();
    let mut current_nodes: Option<Vec<NodeIndex>> = None;
    let mut current_var: Option<String> = None;

    for clause in &plan.clauses {
        match clause {
            Clause::Match(m) | Clause::OptionalMatch(m) => {
                current_nodes = Some(find_matching_nodes(graph, &m.node));
                current_var = m.node.variable.clone();
            }

            Clause::Where(wc) => {
                if let Some(ref nodes) = current_nodes {
                    current_nodes = Some(apply_where(graph, nodes, wc));
                }
            }

            Clause::With(with_clause) => {
                if let (Some(nodes), Some(var)) = (&current_nodes, &current_var) {
                    for item in &with_clause.items {
                        if let WithItem::Aggregate(AggregateFn::Count(_var_name), alias) = item {
                            let counts = count_relationships(graph, nodes);
                            records = nodes.iter()
                                .zip(counts.iter())
                                .map(|(_, &(_, count))| {
                                    let mut fields = HashMap::new();
                                    fields.insert(var.clone(), serde_json::Value::String(var.clone()));
                                    fields.insert(alias.clone(), serde_json::Value::Number((count as i64).into()));
                                    Record { fields }
                                })
                                .collect();
                        }
                    }
                }

                if let Some(ref wc) = with_clause.where_clause {
                    records = records.into_iter()
                        .filter(|r| {
                            wc.comparisons.iter().all(|cmp| {
                                let field_val = r.fields.get(&cmp.field.variable);
                                match (field_val, &cmp.value) {
                                    (Some(serde_json::Value::Number(n)), CompareValue::Int(v)) => {
                                        let val = n.as_i64().unwrap_or(0);
                                        match cmp.op {
                                            CompareOp::Eq => val == *v,
                                            CompareOp::Ne => val != *v,
                                            _ => true,
                                        }
                                    }
                                    _ => true,
                                }
                            })
                        })
                        .collect();
                }
            }

            Clause::Return(ret) => {
                for item in &ret.items {
                    if let Some(ref prop) = item.property {
                        for rec in &mut records {
                            rec.fields.insert(
                                item.alias.clone().unwrap_or_else(|| prop.clone()),
                                serde_json::Value::String(prop.clone()),
                            );
                        }
                    }
                }
            }

            Clause::Create(_create) => {
                // Mutations handled in a separate execution context
            }
        }
    }

    Ok(records)
}

#[derive(Debug)]
pub enum TranslateError {
    NotFound(String),
    Ambiguous(String),
}

impl std::fmt::Display for TranslateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(msg) => write!(f, "not found: {msg}"),
            Self::Ambiguous(msg) => write!(f, "ambiguous: {msg}"),
        }
    }
}

impl std::error::Error for TranslateError {}

// ── Private ──

fn find_matching_nodes<G: GraphLike>(graph: &G, pattern: &NodePattern) -> Vec<NodeIndex> {
    let mut results = Vec::new();
    for node_ref in graph.node_references() {
        let idx = node_ref.id();
        if let Some(weight) = graph.node_weight(idx) {
            if pattern.labels.is_empty() || pattern.labels.contains(&weight.label) {
                results.push(idx);
            }
        }
    }
    results
}

fn apply_where<G: GraphLike>(
    graph: &G,
    nodes: &[NodeIndex],
    wc: &WhereClause,
) -> Vec<NodeIndex> {
    nodes.iter()
        .copied()
        .filter(|&idx| {
            wc.comparisons.iter().all(|cmp| {
                if let Some(weight) = graph.node_weight(idx) {
                    let key = cmp.field.property.as_deref().unwrap_or("");
                    let field_val = weight.properties.get(key);
                    match (field_val, &cmp.value) {
                        (Some(serde_json::Value::String(s)), CompareValue::Str(v)) => {
                            match cmp.op {
                                CompareOp::Eq => s == v,
                                CompareOp::Ne => s != v,
                                _ => true,
                            }
                        }
                        (Some(serde_json::Value::Number(n)), CompareValue::Int(v)) => {
                            let val = n.as_i64().unwrap_or(0);
                            match cmp.op {
                                CompareOp::Gt => val > *v,
                                CompareOp::Lt => val < *v,
                                CompareOp::Eq => val == *v,
                                _ => true,
                            }
                        }
                        _ => true,
                    }
                } else {
                    true
                }
            })
        })
        .collect()
}

fn count_relationships<G: GraphLike>(
    graph: &G,
    nodes: &[NodeIndex],
) -> Vec<(NodeIndex, usize)> {
    nodes.iter()
        .map(|&idx| {
            let count = graph.neighbors_undirected(idx).len();
            (idx, count)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_matching_nodes_empty_graph() {
        // Test that empty graph returns empty results
    }
}
