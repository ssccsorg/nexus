/// Petgraph executor — unified dual-path engine.
///
/// Handles both [`Plan::External`] (cyrs_plan ReadOp chain) and
/// [`Plan::Internal`] (legacy PlanIR, fallback). Default path is
/// always cyrs_plan; internal path exists for robustness.

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

// ── Unified execute ────────────────────────────────────────────────────────

pub fn execute<G: GraphLike>(
    graph: &G,
    plan: &Plan,
) -> Result<Vec<Record>, TranslateError> {
    match plan {
        Plan::External(ext) => execute_external(graph, ext),
        Plan::Internal(ir) => execute_internal(graph, ir),
    }
}

// ── External executor (cyrs_plan ReadOp) ───────────────────────────────────

use cyrs_plan::{self, ReadOp, Expr};

fn execute_external<G: GraphLike>(
    graph: &G,
    plan: &ExternalPlan,
) -> Result<Vec<Record>, TranslateError> {
    // Operator arena: each ReadOp references prior ops via OpId (dense index)
    // We store intermediate row sets per OpId
    let mut row_sets: Vec<RowSet> = Vec::new();

    for op in &plan.ops {
        let rows = exec_readop(graph, op, &row_sets, &plan.var_map)?;
        row_sets.push(rows);
    }

    // Last operator is the root — collect its rows as records
    let last = row_sets.last().cloned().unwrap_or_default();
    Ok(last.into_iter().map(|row| Record { fields: row }).collect())
}

type RowSet = Vec<HashMap<String, serde_json::Value>>;

fn exec_readop<G: GraphLike>(
    graph: &G,
    op: &ReadOp,
    prior: &[RowSet],
    _var_map: &[(cyrs_plan::VarId, String)],
) -> Result<RowSet, TranslateError> {
    match op {
        ReadOp::Source { label, bind } => {
            let labels: Option<&str> = label.as_ref().and_then(|ls| ls.0.first().map(|s| s.as_str()));
            let indices = find_nodes_by_label_str(graph, labels);
            let rows: RowSet = indices.into_iter().map(|idx| {
                let mut row = HashMap::new();
                row.insert(bind.0.to_string(), serde_json::Value::Number((idx.index() as i64).into()));
                row
            }).collect();
            Ok(rows)
        }

        ReadOp::Filter { input, predicate } => {
            let input_rows = get_input(prior, *input)?;
            let kept: RowSet = input_rows.into_iter()
                .filter(|row| is_truthy(&evaluate_expr(graph, row, predicate)))
                .collect();
            Ok(kept)
        }

        ReadOp::Project { input, items } => {
            let input_rows = get_input(prior, *input)?;
            let projected: RowSet = input_rows.into_iter().map(|row| {
                let mut out = HashMap::new();
                for proj in items {
                    let val = evaluate_expr(graph, &row, &proj.expr);
                    let alias = proj.alias.to_string();
                    out.insert(alias, val);
                }
                out
            }).collect();
            Ok(projected)
        }

        ReadOp::Aggregate { input, keys: _, aggs } => {
            let input_rows = get_input(prior, *input)?;
            if input_rows.is_empty() {
                return Ok(vec![]);
            }
            // Simple single-row aggregate (no grouping)
            let mut row = HashMap::new();
            for agg in aggs {
                let count = input_rows.len();
                row.insert(agg.func.to_string(), serde_json::Value::Number((count as i64).into()));
            }
            Ok(vec![row])
        }

        ReadOp::With { input, items, filter } => {
            let input_rows = get_input(prior, *input)?;
            let projected: RowSet = input_rows.into_iter().map(|row| {
                let mut out = HashMap::new();
                for proj in items {
                    let val = evaluate_expr(graph, &row, &proj.expr);
                    out.insert(proj.alias.to_string(), val);
                }
                out
            }).collect();

            let filtered = if let Some(f) = filter {
                projected.into_iter()
                    .filter(|row| is_truthy(&evaluate_expr(graph, row, f)))
                    .collect()
            } else {
                projected
            };
            Ok(filtered)
        }

        ReadOp::Expand { input, from: _, rel: _, to: _, bind_rel, bind_to } => {
            let input_rows = get_input(prior, *input)?;
            let mut expanded = Vec::new();
            for row in input_rows {
                // Find sourced node from row
                let from_idx = find_bound_node(graph, &row);
                if let Some(idx) = from_idx {
                    let neighbors = graph.neighbors_undirected(idx);
                    for neighbor in neighbors {
                        let mut new_row = row.clone();
                        new_row.insert(bind_to.0.to_string(), serde_json::Value::Number((neighbor.index() as i64).into()));
                        if let Some(edge) = find_edge(graph, idx, neighbor) {
                            new_row.insert(bind_rel.0.to_string(), serde_json::Value::Number((edge.index() as i64).into()));
                        }
                        expanded.push(new_row);
                    }
                }
            }
            Ok(expanded)
        }

        _ => Err(TranslateError::Ambiguous(format!(
            "unsupported operator: {:?}", std::mem::discriminant(op)
        ))),
    }
}

fn get_input(prior: &[RowSet], op_id: cyrs_plan::OpId) -> Result<RowSet, TranslateError> {
    prior.get(op_id.0 as usize)
        .cloned()
        .ok_or_else(|| TranslateError::NotFound(format!("OpId {}", op_id.0)))
}

fn evaluate_expr<G: GraphLike>(
    graph: &G,
    row: &HashMap<String, serde_json::Value>,
    expr: &Expr,
) -> serde_json::Value {
    match expr {
        Expr::Var(id) => row.get(&id.0.to_string()).cloned().unwrap_or(serde_json::Value::Null),
        Expr::Int(n) => serde_json::Value::Number((*n).into()),
        Expr::Float(f) => serde_json::json!(*f),
        Expr::String(s) => serde_json::Value::String(s.to_string()),
        Expr::Bool(b) => serde_json::Value::Bool(*b),
        Expr::Null => serde_json::Value::Null,
        Expr::Prop { target, prop } => {
            let node_val = evaluate_expr(graph, row, target);
            if let Some(idx) = node_val.as_i64() {
                let ni = NodeIndex::new(idx as usize);
                if let Some(w) = graph.node_weight(ni) {
                    w.properties.get(prop.as_str()).cloned().unwrap_or(serde_json::Value::Null)
                } else {
                    serde_json::Value::Null
                }
            } else {
                serde_json::Value::Null
            }
        }
        Expr::BinOp { op, lhs, rhs } => {
            let l = evaluate_expr(graph, row, lhs);
            let r = evaluate_expr(graph, row, rhs);
            match op {
                cyrs_plan::BinOp::Eq => serde_json::Value::Bool(l == r),
                cyrs_plan::BinOp::Neq => serde_json::Value::Bool(l != r),
                cyrs_plan::BinOp::Gt => compare_bool(&l, &r, |a, b| a > b),
                cyrs_plan::BinOp::Lt => compare_bool(&l, &r, |a, b| a < b),
                cyrs_plan::BinOp::Ge => compare_bool(&l, &r, |a, b| a >= b),
                cyrs_plan::BinOp::Le => compare_bool(&l, &r, |a, b| a <= b),
                _ => serde_json::Value::Null,
            }
        }
        _ => serde_json::Value::Null,
    }
}

fn compare_bool(l: &serde_json::Value, r: &serde_json::Value, f: fn(f64, f64) -> bool) -> serde_json::Value {
    let lf = value_as_f64(l);
    let rf = value_as_f64(r);
    match (lf, rf) {
        (Some(a), Some(b)) => serde_json::Value::Bool(f(a, b)),
        _ => serde_json::Value::Bool(false),
    }
}

fn is_truthy(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Number(n) => n.as_f64().map_or(false, |f| f != 0.0),
        serde_json::Value::String(s) => !s.is_empty(),
        serde_json::Value::Null => false,
        _ => true,
    }
}

fn value_as_f64(v: &serde_json::Value) -> Option<f64> {
    match v {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn find_bound_node<G: GraphLike>(graph: &G, row: &HashMap<String, serde_json::Value>) -> Option<NodeIndex> {
    for (_, val) in row {
        if let Some(idx) = val.as_i64() {
            let ni = NodeIndex::new(idx as usize);
            if graph.node_weight(ni).is_some() {
                return Some(ni);
            }
        }
    }
    None
}

fn find_edge<G: GraphLike>(
    graph: &G,
    from: NodeIndex,
    to: NodeIndex,
) -> Option<petgraph::graph::EdgeIndex> {
    for (_src, dst, ei) in graph.edges_directed(from, true) {
        if dst == to { return Some(ei); }
    }
    for (_src, dst, ei) in graph.edges_directed(from, false) {
        if dst == to { return Some(ei); }
    }
    None
}

fn find_nodes_by_label_str<G: GraphLike>(graph: &G, label: Option<&str>) -> Vec<NodeIndex> {
    let mut results = Vec::new();
    for node_ref in graph.node_references() {
        let idx = node_ref.id();
        if let Some(weight) = graph.node_weight(idx) {
            if label.map_or(true, |l| weight.label == l) {
                results.push(idx);
            }
        }
    }
    results
}

// ── Internal executor (legacy PlanIR, fallback) ────────────────────────────

fn execute_internal<G: GraphLike>(
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
            Clause::Create(_create) => {}
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

// ── Internal executor helpers ──────────────────────────────────────────────

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

fn apply_where<G: GraphLike>(graph: &G, nodes: &[NodeIndex], wc: &WhereClause) -> Vec<NodeIndex> {
    nodes.iter().copied().filter(|&idx| {
        wc.comparisons.iter().all(|cmp| {
            if let Some(weight) = graph.node_weight(idx) {
                let key = cmp.field.property.as_deref().unwrap_or("");
                let field_val = weight.properties.get(key);
                match (field_val, &cmp.value) {
                    (Some(serde_json::Value::String(s)), CompareValue::Str(v)) => match cmp.op {
                        CompareOp::Eq => s == v,
                        CompareOp::Ne => s != v,
                        _ => true,
                    },
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
            } else { true }
        })
    }).collect()
}

fn count_relationships<G: GraphLike>(graph: &G, nodes: &[NodeIndex]) -> Vec<(NodeIndex, usize)> {
    nodes.iter().map(|&idx| (idx, graph.neighbors_undirected(idx).len())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_matching_nodes_empty_graph() {}
}
