/// Cypher query parser.
///
/// Uses the `cyrs` pipeline (syntax → AST → HIR) internally, then
/// converts to the local [`PlanIR`]. This wrapping layer isolates
/// the rest of `nexus-cypher` from the cyrs dependency — if cyrs is
/// replaced, only this module changes.

use cyrs_hir::{self, Clause as HirClause, Expr, PatternElement};
use crate::plan::*;

/// Parse a Cypher query string into [`PlanIR`].
pub fn parse_query(input: &str) -> Result<PlanIR, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("empty query".to_string());
    }

    let result = cyrs_hir::parse_to_hir(trimmed);
    if !result.syntax_errors.is_empty() {
        let first = &result.syntax_errors[0];
        return Err(format!("parse error at offset {:?}: {}", first.offset, first.message));
    }

    let mut clauses = Vec::new();
    for hir_clause in &result.hir.clauses {
        match hir_clause {
            HirClause::Match { optional: false, pattern, .. } => {
                clauses.push(Clause::Match(hir_match_to_our(pattern, &result.hir)));
            }
            HirClause::Match { optional: true, pattern, .. } => {
                clauses.push(Clause::OptionalMatch(hir_match_to_our(pattern, &result.hir)));
            }
            HirClause::Where { predicate, .. } => {
                clauses.push(Clause::Where(hir_predicate_to_our(predicate)));
            }
            HirClause::With { projections, filter, .. } => {
                clauses.push(Clause::With(hir_with_to_our(projections, filter)));
            }
            HirClause::Return { projections, .. } => {
                clauses.push(Clause::Return(hir_return_to_our(projections)));
            }
            HirClause::Create { pattern, .. } => {
                clauses.push(Clause::Create(hir_create_to_our(pattern, &result.hir)));
            }
            _ => {}
        }
    }

    if clauses.is_empty() {
        return Err("no clauses parsed".to_string());
    }

    Ok(PlanIR { clauses })
}

// ── Variable name resolution ───────────────────────────────────────────────

fn var_name(stmt: &cyrs_hir::Statement, id: cyrs_hir::VarId) -> Option<String> {
    stmt.bindings.get(&id).map(|b| b.name.to_string())
}

// ── Conversion helpers ─────────────────────────────────────────────────────

fn hir_match_to_our(pattern: &cyrs_hir::Pattern, stmt: &cyrs_hir::Statement) -> MatchClause {
    let (node_pat, rel, target) = extract_pattern(pattern, stmt);
    MatchClause {
        node: node_pat,
        relationship: rel,
        target,
    }
}

fn extract_pattern(
    pattern: &cyrs_hir::Pattern,
    stmt: &cyrs_hir::Statement,
) -> (NodePattern, Option<RelPattern>, Option<NodePattern>) {
    for part in &pattern.parts {
        let mut nodes = Vec::new();
        let mut rels = Vec::new();

        for elem in &part.elements {
            match elem {
                PatternElement::Node { bind, labels, .. } => {
                    let variable = bind.and_then(|id| var_name(stmt, id));
                    nodes.push(NodePattern {
                        variable,
                        labels: labels.iter().map(|s| s.to_string()).collect(),
                    });
                }
                PatternElement::Rel { bind, types, direction, .. } => {
                    let dir = match direction {
                        cyrs_hir::Direction::Outgoing => Direction::Outgoing,
                        cyrs_hir::Direction::Incoming => Direction::Incoming,
                        cyrs_hir::Direction::Undirected => Direction::Both,
                            _ => Direction::Both,
                    };
                    rels.push(RelPattern {
                        variable: bind.and_then(|id| var_name(stmt, id)),
                        types: types.iter().map(|s| s.to_string()).collect(),
                        direction: dir,
                    });
                }
            }
        }

        let source = nodes.first().cloned().unwrap_or(NodePattern {
            variable: None,
            labels: Vec::new(),
        });
        let rel = rels.first().cloned();
        let target = if rel.is_some() && nodes.len() > 1 {
            nodes.get(1).cloned()
        } else {
            None
        };

        return (source, rel, target);
    }

    (NodePattern { variable: None, labels: Vec::new() }, None, None)
}

fn hir_predicate_to_our(expr: &Expr) -> WhereClause {
    let mut comparisons = Vec::new();
    extract_comparisons(expr, &mut comparisons);
    WhereClause {
        field_eq: Vec::new(),
        not_exists: None,
        comparisons,
    }
}

fn extract_comparisons(expr: &Expr, out: &mut Vec<Comparison>) {
    match expr {
        Expr::BinOp { op, lhs, rhs } => {
            let op_enum = match op {
                cyrs_hir::BinOp::Eq => CompareOp::Eq,
                cyrs_hir::BinOp::Neq => CompareOp::Ne,
                cyrs_hir::BinOp::Gt => CompareOp::Gt,
                cyrs_hir::BinOp::Lt => CompareOp::Lt,
                cyrs_hir::BinOp::Ge => CompareOp::Gte,
                cyrs_hir::BinOp::Le => CompareOp::Lte,
                _ => return,
            };

            if let (Some(field), Some(value)) = (extract_field_ref(lhs), extract_value(rhs)) {
                out.push(Comparison { field, op: op_enum, value });
            }
        }
        _ => {}
    }
}

fn extract_field_ref(expr: &Expr) -> Option<FieldRef> {
    match expr {
        Expr::Prop { target, prop } => {
            let var = extract_var_name(target).unwrap_or_default();
            Some(FieldRef {
                variable: var,
                property: Some(prop.to_string()),
            })
        }
        Expr::Var(_) | Expr::String(_) => {
            // Treat as direct variable reference
            let name = expr_to_string(expr)?;
            Some(FieldRef { variable: name, property: None })
        }
        Expr::Unresolved(s) => {
            Some(FieldRef { variable: s.to_string(), property: None })
        }
        _ => None,
    }
}

fn extract_var_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Var(_) => None,
        Expr::String(s) => Some(s.to_string()),
        Expr::Prop { target, prop: _ } => extract_var_name(target),
        _ => None,
    }
}

fn extract_value(expr: &Expr) -> Option<CompareValue> {
    match expr {
        Expr::Var(_) => {
            let name = String::new();
            // Try numeric parse
            if let Ok(n) = name.parse::<i64>() {
                return Some(CompareValue::Int(n));
            }
            if name.contains('.') {
                if let Ok(f) = name.parse::<f64>() {
                    return Some(CompareValue::Float(f));
                }
            }
            Some(CompareValue::Field(FieldRef {
                variable: name,
                property: None,
            }))
        }
        Expr::String(s) => {
            let name = s.to_string();
            // Try numeric parse
            if let Ok(n) = name.parse::<i64>() {
                return Some(CompareValue::Int(n));
            }
            if name.contains('.') {
                if let Ok(f) = name.parse::<f64>() {
                    return Some(CompareValue::Float(f));
                }
            }
            Some(CompareValue::Field(FieldRef {
                variable: name,
                property: None,
            }))
        }
        Expr::Prop { target, prop } => {
            let var = extract_var_name(target).unwrap_or_default();
            Some(CompareValue::Field(FieldRef {
                variable: var,
                property: Some(prop.to_string()),
            }))
        }
        Expr::Int(n) => Some(CompareValue::Int(*n)),
        Expr::Float(f) => Some(CompareValue::Float(*f)),
        _ => None,
    }
}

fn expr_to_string(expr: &Expr) -> Option<String> {
    match expr {
        Expr::String(s) => Some(s.to_string()),
        Expr::Var(id) => Some(format!("v{}", id.0)),
        Expr::Prop { target, prop } => {
            let base = expr_to_string(target)?;
            Some(format!("{}.{}", base, prop))
        }
        Expr::Int(n) => Some(n.to_string()),
        Expr::Float(f) => Some(f.to_string()),
        _ => None,
    }
}

fn hir_with_to_our(projections: &[cyrs_hir::Projection], filter: &Option<Expr>) -> WithClause {
    let mut items = Vec::new();
    for proj in projections {
        items.push(hir_proj_to_with_item(proj));
    }
    let where_clause = filter.as_ref().map(|e| hir_predicate_to_our(e));
    WithClause { items, where_clause }
}

fn hir_proj_to_with_item(proj: &cyrs_hir::Projection) -> WithItem {
    let alias = proj.alias.as_ref().map(|s| s.to_string()).unwrap_or_default();
    match &proj.expr {
        Expr::Call { name, args, .. } if name == "count" => {
            let var = args.first()
                .and_then(|a| expr_to_string(a))
                .unwrap_or_default();
            WithItem::Aggregate(AggregateFn::Count(var), alias)
        }
        _ => WithItem::Var(alias),
    }
}

fn hir_return_to_our(projections: &[cyrs_hir::Projection]) -> ReturnClause {
    let items = projections.iter().map(|proj| {
        let property = match &proj.expr {
            Expr::Prop { target: _, prop } => Some(prop.to_string()),
            _ => None,
        };
        let alias = proj.alias.as_ref().map(|s| s.to_string());
        ReturnItem { property, alias }
    }).collect();
    ReturnClause { items }
}

fn hir_create_to_our(pattern: &cyrs_hir::Pattern, stmt: &cyrs_hir::Statement) -> CreateClause {
    let mut nodes = Vec::new();
    for part in &pattern.parts {
        for elem in &part.elements {
            if let PatternElement::Node { bind, labels, props, .. } = elem {
                let np = NodePattern {
                    variable: bind.and_then(|id| var_name(stmt, id)),
                    labels: labels.iter().map(|s| s.to_string()).collect(),
                };
                let properties = props.as_ref()
                    .map(|p| extract_properties(p))
                    .unwrap_or_default();
                nodes.push((np, properties));
            }
        }
    }
    CreateClause { nodes }
}

fn extract_properties(expr: &Expr) -> Vec<(String, PropertyValue)> {
    match expr {
        Expr::Map(pairs) => {
            pairs.iter().filter_map(|(k, v)| {
                let pv = expr_to_prop_value(v)?;
                Some((k.to_string(), pv))
            }).collect()
        }
        _ => Vec::new(),
    }
}

fn expr_to_prop_value(expr: &Expr) -> Option<PropertyValue> {
    match expr {
        Expr::Int(n) => Some(PropertyValue::Int(*n)),
        Expr::Float(f) => Some(PropertyValue::Float(*f)),
        Expr::String(s) => Some(PropertyValue::Str(s.to_string())),
        Expr::Bool(b) => Some(PropertyValue::Bool(*b)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_match() {
        let plan = parse_query("MATCH (c:Concept) RETURN c").unwrap();
        assert_eq!(plan.clauses.len(), 2);
        match &plan.clauses[0] {
            Clause::Match(m) => {
                assert_eq!(m.node.variable, Some("c".into()));
                assert!(m.node.labels.contains(&"Concept".to_string()));
            }
            _ => panic!("expected Match"),
        }
    }

    #[test]
    fn test_parse_match_relationship() {
        let input = "MATCH (a)-[r]->(b) RETURN a, b";
        let plan = parse_query(input).unwrap();
        match &plan.clauses[0] {
            Clause::Match(m) => {
                assert!(m.relationship.is_some());
                assert!(m.target.is_some());
            }
            _ => panic!("expected Match"),
        }
    }

    #[test]
    fn test_parse_optional_match() {
        let input = "OPTIONAL MATCH (c)-[r]-() WITH c, count(r) AS rc WHERE rc = 0";
        let plan = parse_query(input).unwrap();
        assert_eq!(plan.clauses.len(), 2, "{:?}", plan.clauses);
        match &plan.clauses[0] {
            Clause::OptionalMatch(_) => {}
            _ => panic!("expected OptionalMatch"),
        }
        match &plan.clauses[1] {
            Clause::With(w) => {
                assert!(!w.items.is_empty());
            }
            _ => panic!("expected With"),
        }
    }

    #[test]
    fn test_parse_create() {
        let input = "CREATE (n:Person {name: \"Alice\", age: 30})";
        let plan = parse_query(input).unwrap();
        match &plan.clauses[0] {
            Clause::Create(c) => {
                assert_eq!(c.nodes.len(), 1);
                assert_eq!(c.nodes[0].0.variable, Some("n".into()));
                assert!(c.nodes[0].0.labels.contains(&"Person".to_string()));
                assert!(!c.nodes[0].1.is_empty(), "expected properties");
            }
            _ => panic!("expected Create"),
        }
    }

    #[test]
    fn test_parse_empty() {
        let result = parse_query("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_where_condition() {
        let input = "MATCH (c:Concept) WHERE c.value > 5 RETURN c.name";
        let plan = parse_query(input).unwrap();
        assert_eq!(plan.clauses.len(), 3, "{:?}", plan.clauses);
        match &plan.clauses[1] {
            Clause::Where(wc) => {
                assert!(!wc.comparisons.is_empty());
            }
            _ => panic!("expected Where"),
        }
    }
}
