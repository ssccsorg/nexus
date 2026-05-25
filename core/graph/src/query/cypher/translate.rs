/// Petgraph executor — unified dual-path engine.
///
/// Handles both [`Plan::External`] (cyrs_plan ReadOp chain) and
/// [`Plan::Internal`] (legacy PlanIR, fallback). Default path is
/// always cyrs_plan; internal path exists for robustness.
///
/// All query functions take `&impl GraphRead`, so callers can pass
/// either a `DefaultBlackboard` directly or an `RwLockReadGuard`
/// obtained via `DefaultBlackboard::snapshot()`. The latter is
/// preferred for batch operations since it acquires the graph read
/// lock only once for the entire query execution.
use petgraph::graph::NodeIndex;

use std::collections::HashMap;

use super::plan::*;

use nexus_model::CypherCapable;
use nexus_storage_petgraph::{GraphRead, Record};

// ── Unified execute ────────────────────────────────────────────────────────

/// Execute a query plan against the hot petgraph only.
///
/// This is the hot-only execution path. For production use with
/// hot/cold routing, prefer [`execute_with_cold`] which automatically
/// routes cold-eligible queries to the cold storage backend.
pub fn execute<G: GraphRead>(graph: &G, plan: &Plan) -> Result<Vec<Record>, TranslateError> {
    match plan {
        Plan::External(ext) => execute_external(graph, ext),
        Plan::Internal(ir) => execute_internal(graph, ir),
    }
}

/// Execute a query plan with hot/cold routing.
///
/// If the plan can be expressed as a `ColdQuery`, it is routed to the cold
/// storage backend via `CypherCapable::query_plan()`. Otherwise, it falls
/// back to the hot petgraph executor.
///
/// This is the preferred entry point for production use, where `DualStorage`
/// or a `DuckDbStorage` instance is available as the cold backend.
pub fn execute_with_cold<G: GraphRead, C: CypherCapable>(
    graph: &G,
    cold: &C,
    plan: &Plan,
) -> Result<Vec<Record>, TranslateError> {
    // Try cold routing first.
    if let Some(cold_query) = plan.to_cold_query() {
        let plan_json =
            serde_json::to_value(&cold_query).map_err(|e| TranslateError::Other(e.to_string()))?;
        let result = cold.query_plan(&plan_json).map_err(TranslateError::Other)?;
        // Parse the JSON array result into Vec<Record>.
        let records: Vec<Record> = if let serde_json::Value::Array(arr) = result {
            arr.into_iter()
                .map(|item| {
                    let fields = if let serde_json::Value::Object(obj) = item {
                        obj.into_iter().collect()
                    } else {
                        HashMap::new()
                    };
                    Record { fields }
                })
                .collect()
        } else {
            return Err(TranslateError::Other(
                "cold query result is not an array".into(),
            ));
        };
        return Ok(records);
    }

    // Fallback to hot petgraph executor.
    execute(graph, plan)
}

// ── External executor (cyrs_plan ReadOp) ───────────────────────────────────

use cyrs_plan::{self, Expr, ReadOp};

fn execute_external<G: GraphRead>(
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

fn exec_readop<G: GraphRead>(
    graph: &G,
    op: &ReadOp,
    prior: &[RowSet],
    _var_map: &[(cyrs_plan::VarId, String)],
) -> Result<RowSet, TranslateError> {
    match op {
        ReadOp::Source { label, bind } => {
            let labels: Option<&str> = label
                .as_ref()
                .and_then(|ls| ls.0.first().map(|s| s.as_str()));
            let indices = find_nodes_by_label_str(graph, labels);
            let rows: RowSet = indices
                .into_iter()
                .map(|idx| {
                    let mut row = HashMap::new();
                    row.insert(
                        bind.0.to_string(),
                        serde_json::Value::Number((idx.index() as i64).into()),
                    );
                    row
                })
                .collect();
            Ok(rows)
        }

        ReadOp::Filter { input, predicate } => {
            let input_rows = get_input(prior, *input)?;
            let kept: RowSet = input_rows
                .into_iter()
                .filter(|row| is_truthy(&evaluate_expr(graph, row, predicate)))
                .collect();
            Ok(kept)
        }

        ReadOp::Project { input, items } => {
            let input_rows = get_input(prior, *input)?;
            let projected: RowSet = input_rows
                .into_iter()
                .map(|row| {
                    let mut out = HashMap::new();
                    for proj in items {
                        let val = evaluate_expr(graph, &row, &proj.expr);
                        let alias = proj.alias.to_string();
                        out.insert(alias, val);
                    }
                    out
                })
                .collect();
            Ok(projected)
        }

        ReadOp::Aggregate {
            input,
            keys: _,
            aggs,
        } => {
            let input_rows = get_input(prior, *input)?;
            if input_rows.is_empty() {
                return Ok(vec![]);
            }
            // Simple single-row aggregate (no grouping)
            let mut row = HashMap::new();
            for agg in aggs {
                let count = input_rows.len();
                row.insert(
                    agg.func.to_string(),
                    serde_json::Value::Number((count as i64).into()),
                );
            }
            Ok(vec![row])
        }

        ReadOp::With {
            input,
            items,
            filter,
        } => {
            let input_rows = get_input(prior, *input)?;
            let projected: RowSet = input_rows
                .into_iter()
                .map(|row| {
                    let mut out = HashMap::new();
                    for proj in items {
                        let val = evaluate_expr(graph, &row, &proj.expr);
                        out.insert(proj.alias.to_string(), val);
                    }
                    out
                })
                .collect();

            let filtered = if let Some(f) = filter {
                projected
                    .into_iter()
                    .filter(|row| is_truthy(&evaluate_expr(graph, row, f)))
                    .collect()
            } else {
                projected
            };
            Ok(filtered)
        }

        ReadOp::Expand {
            input,
            from: _,
            rel,
            to: _,
            bind_rel,
            bind_to,
        } => {
            let input_rows = get_input(prior, *input)?;
            let rel_types: Vec<&str> = rel.types.iter().map(|s| s.as_str()).collect();
            let dir = &rel.direction;

            let mut expanded = Vec::new();
            for row in input_rows {
                let from_idx = find_bound_node(graph, &row);
                if let Some(idx) = from_idx {
                    // Use directed edge traversal based on direction
                    let neighbors = match dir {
                        cyrs_plan::Direction::Outgoing => graph
                            .edges_directed(idx, true)
                            .into_iter()
                            .filter_map(|ei| graph.edge_endpoints(ei).map(|(_, dst)| dst))
                            .collect(),
                        cyrs_plan::Direction::Incoming => graph
                            .edges_directed(idx, false)
                            .into_iter()
                            .filter_map(|ei| graph.edge_endpoints(ei).map(|(_, dst)| dst))
                            .collect(),
                        cyrs_plan::Direction::Undirected => graph.neighbors_undirected(idx),
                        _ => graph.neighbors_undirected(idx),
                    };

                    // Filter edges by type if specified
                    for neighbor in neighbors {
                        // Check edge type match
                        let edge_idx = find_edge_filtered(graph, idx, neighbor, &rel_types, dir);
                        if !rel_types.is_empty() && edge_idx.is_none() {
                            continue;
                        }

                        let mut new_row = row.clone();
                        new_row.insert(
                            bind_to.0.to_string(),
                            serde_json::Value::Number((neighbor.index() as i64).into()),
                        );
                        if let Some(ei) = edge_idx.or_else(|| find_edge(graph, idx, neighbor)) {
                            new_row.insert(
                                bind_rel.0.to_string(),
                                serde_json::Value::Number((ei.index() as i64).into()),
                            );
                        }
                        expanded.push(new_row);
                    }
                }
            }
            Ok(expanded)
        }

        ReadOp::OrderBy { input, keys } => {
            let mut rows = get_input(prior, *input)?;
            rows.sort_by(|a, b| {
                for key in keys {
                    let va = evaluate_expr(graph, a, &key.expr);
                    let vb = evaluate_expr(graph, b, &key.expr);
                    let cmp = cmp_values(&va, &vb);
                    if cmp != std::cmp::Ordering::Equal {
                        return match key.dir {
                            cyrs_plan::SortDir::Asc => cmp,
                            cyrs_plan::SortDir::Desc => cmp.reverse(),
                            _ => cmp,
                        };
                    }
                }
                std::cmp::Ordering::Equal
            });
            Ok(rows)
        }

        ReadOp::Limit { input, count } => {
            let rows = get_input(prior, *input)?;
            let limit = eval_expr_as_usize(count);
            Ok(rows.into_iter().take(limit).collect())
        }

        ReadOp::Skip { input, count } => {
            let rows = get_input(prior, *input)?;
            let skip = eval_expr_as_usize(count);
            Ok(rows.into_iter().skip(skip).collect())
        }

        ReadOp::Distinct { input } => {
            let mut rows = get_input(prior, *input)?;
            // Simple dedup by row content (key order independent)
            let mut seen: Vec<HashMap<String, serde_json::Value>> = Vec::new();
            rows.retain(|row| {
                let is_new = !seen.iter().any(|s| rows_equal(s, row));
                if is_new {
                    seen.push(row.clone());
                }
                is_new
            });
            Ok(rows)
        }

        ReadOp::OptionalJoin { input, pattern } => {
            let input_rows = get_input(prior, *input)?;
            let mut result = Vec::new();

            // Recursively execute the inner pattern for each input row
            let sub_arena: &[ReadOp] = std::slice::from_ref(pattern);
            let mut sub_row_sets: Vec<RowSet> = Vec::new();

            for row in input_rows {
                sub_row_sets.clear();

                // Execute inner pattern starting from this row
                let mut success = true;
                for sub_op in sub_arena {
                    let sub_rows = exec_readop(graph, sub_op, &sub_row_sets, _var_map)?;
                    if sub_rows.is_empty() {
                        success = false;
                        break;
                    }
                    sub_row_sets.push(sub_rows);
                }

                if success {
                    if let Some(inner_rows) = sub_row_sets.last().cloned() {
                        for inner in inner_rows {
                            let mut merged = row.clone();
                            merged.extend(inner);
                            result.push(merged);
                        }
                    }
                } else {
                    // No match: emit input row with nulls for bound vars
                    result.push(row);
                }
            }
            Ok(result)
        }

        _ => Err(TranslateError::Ambiguous(format!(
            "unsupported operator: {:?}",
            std::mem::discriminant(op)
        ))),
    }
}

fn get_input(prior: &[RowSet], op_id: cyrs_plan::OpId) -> Result<RowSet, TranslateError> {
    prior
        .get(op_id.0 as usize)
        .cloned()
        .ok_or_else(|| TranslateError::NotFound(format!("OpId {}", op_id.0)))
}

fn evaluate_expr<G: GraphRead>(
    graph: &G,
    row: &HashMap<String, serde_json::Value>,
    expr: &Expr,
) -> serde_json::Value {
    match expr {
        Expr::Var(id) => row
            .get(&id.0.to_string())
            .cloned()
            .unwrap_or(serde_json::Value::Null),
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
                    w.properties
                        .get(prop.as_str())
                        .cloned()
                        .unwrap_or(serde_json::Value::Null)
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

fn compare_bool(
    l: &serde_json::Value,
    r: &serde_json::Value,
    f: fn(f64, f64) -> bool,
) -> serde_json::Value {
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
        serde_json::Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
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

fn find_bound_node<G: GraphRead>(
    graph: &G,
    row: &HashMap<String, serde_json::Value>,
) -> Option<NodeIndex> {
    for val in row.values() {
        if let Some(idx) = val.as_i64() {
            let ni = NodeIndex::new(idx as usize);
            if graph.node_weight(ni).is_some() {
                return Some(ni);
            }
        }
    }
    None
}

fn find_edge<G: GraphRead>(
    graph: &G,
    from: NodeIndex,
    to: NodeIndex,
) -> Option<petgraph::graph::EdgeIndex> {
    for &ei in &graph.edges_directed(from, true) {
        if let Some((_, dst)) = graph.edge_endpoints(ei)
            && dst == to
        {
            return Some(ei);
        }
    }
    for &ei in &graph.edges_directed(from, false) {
        if let Some((_, dst)) = graph.edge_endpoints(ei)
            && dst == to
        {
            return Some(ei);
        }
    }
    None
}

fn find_edge_filtered<G: GraphRead>(
    graph: &G,
    from: NodeIndex,
    to: NodeIndex,
    types: &[&str],
    _dir: &cyrs_plan::Direction,
) -> Option<petgraph::graph::EdgeIndex> {
    for &ei in &graph.edges_directed(from, true) {
        if let Some((_, dst)) = graph.edge_endpoints(ei)
            && dst == to
        {
            if let Some(ew) = graph.edge_weight(ei) {
                if types.is_empty() || types.contains(&ew.rel_type.as_str()) {
                    return Some(ei);
                }
            } else if types.is_empty() {
                return Some(ei);
            }
        }
    }
    for &ei in &graph.edges_directed(from, false) {
        if let Some((_, dst)) = graph.edge_endpoints(ei)
            && dst == to
        {
            if let Some(ew) = graph.edge_weight(ei) {
                if types.is_empty() || types.contains(&ew.rel_type.as_str()) {
                    return Some(ei);
                }
            } else if types.is_empty() {
                return Some(ei);
            }
        }
    }
    None
}

fn eval_expr_as_usize(expr: &Expr) -> usize {
    match expr {
        Expr::Int(n) => *n as usize,
        _ => 0,
    }
}

fn cmp_values(a: &serde_json::Value, b: &serde_json::Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (serde_json::Value::Number(na), serde_json::Value::Number(nb)) => {
            let fa = na.as_f64().unwrap_or(0.0);
            let fb = nb.as_f64().unwrap_or(0.0);
            fa.partial_cmp(&fb).unwrap_or(Ordering::Equal)
        }
        (serde_json::Value::String(sa), serde_json::Value::String(sb)) => sa.cmp(sb),
        (serde_json::Value::Bool(ba), serde_json::Value::Bool(bb)) => ba.cmp(bb),
        (serde_json::Value::Null, serde_json::Value::Null) => Ordering::Equal,
        (serde_json::Value::Null, _) => Ordering::Less,
        (_, serde_json::Value::Null) => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

fn rows_equal(
    a: &HashMap<String, serde_json::Value>,
    b: &HashMap<String, serde_json::Value>,
) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().all(|(k, v)| b.get(k) == Some(v))
}

fn find_nodes_by_label_str<G: GraphRead>(graph: &G, label: Option<&str>) -> Vec<NodeIndex> {
    let mut results = Vec::new();
    for &idx in &graph.node_indices() {
        if let Some(weight) = graph.node_weight(idx)
            && label.is_none_or(|l| weight.label == l)
        {
            results.push(idx);
        }
    }
    results
}

// ── Internal executor (legacy PlanIR, fallback) ────────────────────────────

fn execute_internal<G: GraphRead>(graph: &G, plan: &PlanIR) -> Result<Vec<Record>, TranslateError> {
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
                            records = nodes
                                .iter()
                                .zip(counts.iter())
                                .map(|(_, &(_, count))| {
                                    let mut fields = HashMap::new();
                                    fields.insert(
                                        var.clone(),
                                        serde_json::Value::String(var.clone()),
                                    );
                                    fields.insert(
                                        alias.clone(),
                                        serde_json::Value::Number((count as i64).into()),
                                    );
                                    Record { fields }
                                })
                                .collect();
                        }
                    }
                }
                if let Some(ref wc) = with_clause.where_clause {
                    records.retain(|r| {
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
                    });
                }
            }
            Clause::Return(ret) => {
                if records.is_empty() {
                    // MATCH ... RETURN: create records from current_nodes
                    if let (Some(nodes), Some(var)) = (&current_nodes, &current_var) {
                        records = nodes
                            .iter()
                            .map(|&idx| {
                                let mut fields = HashMap::new();
                                fields.insert(var.clone(), serde_json::Value::String(var.clone()));
                                if let Some(w) = graph.node_weight(idx) {
                                    for (k, v) in &w.properties {
                                        fields.insert(k.clone(), v.clone());
                                    }
                                }
                                Record { fields }
                            })
                            .collect();
                    }
                }
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
    Other(String),
}

impl std::fmt::Display for TranslateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(msg) => write!(f, "not found: {msg}"),
            Self::Ambiguous(msg) => write!(f, "ambiguous: {msg}"),
            Self::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for TranslateError {}

// ── Internal executor helpers ──────────────────────────────────────────────

fn find_matching_nodes<G: GraphRead>(graph: &G, pattern: &NodePattern) -> Vec<NodeIndex> {
    let mut results = Vec::new();
    for &idx in &graph.node_indices() {
        if let Some(weight) = graph.node_weight(idx)
            && (pattern.labels.is_empty() || pattern.labels.contains(&weight.label))
        {
            results.push(idx);
        }
    }
    results
}

fn apply_where<G: GraphRead>(graph: &G, nodes: &[NodeIndex], wc: &WhereClause) -> Vec<NodeIndex> {
    nodes
        .iter()
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

fn count_relationships<G: GraphRead>(graph: &G, nodes: &[NodeIndex]) -> Vec<(NodeIndex, usize)> {
    nodes
        .iter()
        .map(|&idx| (idx, graph.neighbors_undirected(idx).len()))
        .collect()
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_find_matching_nodes_empty_graph() {}
}
