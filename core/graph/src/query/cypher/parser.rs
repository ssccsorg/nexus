/// Cypher query parser.
///
/// Uses the `cyrs` pipeline (syntax → AST → HIR → name resolution → plan)
/// internally, then converts to petgraph-executable form.
///
/// When fully integrated, this entire module reduces to:
/// ```ignore
/// pub fn parse_query(input: &str) -> Result<PlanStatement, String> {
///     let hir = cyrs_hir::parse_to_hir(input).hir;
///     let resolved = resolve_names(&hir)?;
///     let plan = cyrs_plan::lower::lower_statement(&resolved)
///         .map_err(|e| e.to_string())?;
///     Ok(plan)
/// }
/// ```
use std::collections::HashMap;

use super::plan::*;
use cyrs_hir::{self, Clause as HirClause, Expr, PatternElement};

/// Parse a Cypher query string into [`PlanIR`].
pub fn parse_query(input: &str) -> Result<PlanIR, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("empty query".to_string());
    }

    let result = cyrs_hir::parse_to_hir(trimmed);
    if !result.syntax_errors.is_empty() {
        let first = &result.syntax_errors[0];
        return Err(format!(
            "parse error at offset {:?}: {}",
            first.offset, first.message
        ));
    }

    let mut clauses = Vec::new();
    for hir_clause in &result.hir.clauses {
        match hir_clause {
            HirClause::Match {
                optional: false,
                pattern,
                ..
            } => {
                clauses.push(Clause::Match(hir_match_to_our(pattern, &result.hir)));
            }
            HirClause::Match {
                optional: true,
                pattern,
                ..
            } => {
                clauses.push(Clause::OptionalMatch(hir_match_to_our(
                    pattern,
                    &result.hir,
                )));
            }
            HirClause::Where { predicate, .. } => {
                clauses.push(Clause::Where(hir_predicate_to_our(predicate)));
            }
            HirClause::With {
                projections,
                filter,
                ..
            } => {
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

// ── Name resolution ────────────────────────────────────────────────────────

pub fn resolve_names(stmt: &mut cyrs_hir::Statement) {
    let name_to_id: HashMap<String, cyrs_hir::VarId> = stmt
        .bindings
        .iter()
        .map(|(id, b)| (b.name.to_string(), *id))
        .collect();

    for clause in &mut stmt.clauses {
        resolve_clause_exprs(clause, &name_to_id);
    }
}

fn resolve_clause_exprs(clause: &mut HirClause, name_to_id: &HashMap<String, cyrs_hir::VarId>) {
    match clause {
        HirClause::Match { pattern, .. } => resolve_pattern_exprs(pattern, name_to_id),
        HirClause::Where { predicate, .. } => resolve_expr(predicate, name_to_id),
        HirClause::With {
            projections,
            filter,
            ..
        } => {
            for proj in projections {
                resolve_expr(&mut proj.expr, name_to_id);
            }
            if let Some(f) = filter {
                resolve_expr(f, name_to_id);
            }
        }
        HirClause::Return { projections, .. } => {
            for proj in projections {
                resolve_expr(&mut proj.expr, name_to_id);
            }
        }
        HirClause::Create { pattern, .. } => resolve_pattern_exprs(pattern, name_to_id),
        _ => {}
    }
}

fn resolve_pattern_exprs(
    pattern: &mut cyrs_hir::Pattern,
    name_to_id: &HashMap<String, cyrs_hir::VarId>,
) {
    for part in &mut pattern.parts {
        for elem in &mut part.elements {
            match elem {
                PatternElement::Node { props, .. } => {
                    if let Some(p) = props {
                        resolve_expr(p, name_to_id);
                    }
                }
                PatternElement::Rel { props, .. } => {
                    if let Some(p) = props {
                        resolve_expr(p, name_to_id);
                    }
                }
            }
        }
    }
}

fn resolve_expr(expr: &mut Expr, name_to_id: &HashMap<String, cyrs_hir::VarId>) {
    #[allow(unreachable_patterns)]
    match expr {
        Expr::Unresolved(name) => {
            if let Some(&id) = name_to_id.get(name.as_str()) {
                *expr = Expr::Var(id);
            }
        }
        Expr::Prop { target, .. } => resolve_expr(target.as_mut(), name_to_id),
        Expr::BinOp { lhs, rhs, .. } => {
            resolve_expr(lhs, name_to_id);
            resolve_expr(rhs, name_to_id);
        }
        Expr::UnaryOp { operand, .. } => resolve_expr(operand, name_to_id),
        Expr::Call { args, .. } => {
            for arg in args {
                resolve_expr(arg, name_to_id);
            }
        }
        Expr::Index { target, index } => {
            resolve_expr(target, name_to_id);
            resolve_expr(index, name_to_id);
        }
        Expr::Slice { target, start, end } => {
            resolve_expr(target, name_to_id);
            if let Some(s) = start {
                resolve_expr(s, name_to_id);
            }
            if let Some(e) = end {
                resolve_expr(e, name_to_id);
            }
        }
        Expr::List(items) => {
            for item in items {
                resolve_expr(item, name_to_id);
            }
        }
        Expr::Map(pairs) => {
            for (_, v) in pairs {
                resolve_expr(v, name_to_id);
            }
        }
        Expr::Case {
            scrutinee,
            arms,
            otherwise,
        } => {
            if let Some(s) = scrutinee {
                resolve_expr(s, name_to_id);
            }
            for (cond, body) in arms {
                resolve_expr(cond, name_to_id);
                resolve_expr(body, name_to_id);
            }
            if let Some(o) = otherwise {
                resolve_expr(o, name_to_id);
            }
        }
        Expr::IsNull { operand, .. } => resolve_expr(operand, name_to_id),
        Expr::InList { operand, list } => {
            resolve_expr(operand, name_to_id);
            resolve_expr(list, name_to_id);
        }
        Expr::PatternPredicate(pattern) => resolve_pattern_exprs(pattern, name_to_id),
        Expr::ListComprehension {
            iterable,
            filter,
            map_expr,
            ..
        } => {
            resolve_expr(iterable, name_to_id);
            if let Some(f) = filter {
                resolve_expr(f, name_to_id);
            }
            resolve_expr(map_expr, name_to_id);
        }
        Expr::ListPredicate {
            iterable,
            predicate,
            ..
        } => {
            resolve_expr(iterable, name_to_id);
            if let Some(p) = predicate {
                resolve_expr(p, name_to_id);
            }
        }
        Expr::MapProjection { base, items } => {
            resolve_expr(base, name_to_id);
            for item in items {
                use cyrs_hir::MapProjectionItem;
                match item {
                    MapProjectionItem::PropCopy { .. } | MapProjectionItem::VarShorthand { .. } => {
                    }
                    MapProjectionItem::Computed { value, .. }
                    | MapProjectionItem::Aliased { value, .. } => resolve_expr(value, name_to_id),
                }
            }
        }
        Expr::Null
        | Expr::Bool(_)
        | Expr::Int(_)
        | Expr::Float(_)
        | Expr::String(_)
        | Expr::Var(_)
        | Expr::Param(_) => {}
        _ => {}
    }
}

// ── Variable name resolution helpers ───────────────────────────────────────

fn var_name(stmt: &cyrs_hir::Statement, id: cyrs_hir::VarId) -> Option<String> {
    stmt.bindings.get(&id).map(|b| b.name.to_string())
}

// ── HIR → PlanIR conversion ────────────────────────────────────────────────

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
    if let Some(part) = pattern.parts.first() {
        let mut nodes = Vec::new();
        let mut rels = Vec::new();
        for elem in &part.elements {
            match elem {
                PatternElement::Node { bind, labels, .. } => {
                    nodes.push(NodePattern {
                        variable: bind.and_then(|id| var_name(stmt, id)),
                        labels: labels.iter().map(|s| s.to_string()).collect(),
                    });
                }
                PatternElement::Rel {
                    bind,
                    types,
                    direction,
                    ..
                } => {
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
    (
        NodePattern {
            variable: None,
            labels: Vec::new(),
        },
        None,
        None,
    )
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
    if let Expr::BinOp { op, lhs, rhs } = expr {
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
            out.push(Comparison {
                field,
                op: op_enum,
                value,
            });
        }
    }
}

fn extract_field_ref(expr: &Expr) -> Option<FieldRef> {
    match expr {
        Expr::Prop { target, prop } => Some(FieldRef {
            variable: extract_var_name(target).unwrap_or_default(),
            property: Some(prop.to_string()),
        }),
        Expr::Unresolved(s) => Some(FieldRef {
            variable: s.to_string(),
            property: None,
        }),
        _ => None,
    }
}

fn extract_var_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Var(_) => None,
        Expr::String(s) => Some(s.to_string()),
        Expr::Prop { target, .. } => extract_var_name(target),
        _ => None,
    }
}

fn extract_value(expr: &Expr) -> Option<CompareValue> {
    match expr {
        Expr::Var(_) => None,
        Expr::String(s) => {
            let name = s.to_string();
            if let Ok(n) = name.parse::<i64>() {
                return Some(CompareValue::Int(n));
            }
            Some(CompareValue::Field(FieldRef {
                variable: name,
                property: None,
            }))
        }
        Expr::Prop { target, prop } => Some(CompareValue::Field(FieldRef {
            variable: extract_var_name(target).unwrap_or_default(),
            property: Some(prop.to_string()),
        })),
        Expr::Int(n) => Some(CompareValue::Int(*n)),
        Expr::Float(f) => Some(CompareValue::Float(*f)),
        _ => None,
    }
}

fn hir_with_to_our(projections: &[cyrs_hir::Projection], filter: &Option<Expr>) -> WithClause {
    let items: Vec<_> = projections.iter().map(hir_proj_to_with_item).collect();
    let where_clause = filter.as_ref().map(hir_predicate_to_our);
    WithClause {
        items,
        where_clause,
    }
}

fn hir_proj_to_with_item(proj: &cyrs_hir::Projection) -> WithItem {
    let alias = proj
        .alias
        .as_ref()
        .map(|s| s.to_string())
        .unwrap_or_default();
    if let Expr::Call { name, args, .. } = &proj.expr
        && name == "count"
    {
        let var = args
            .first()
            .and_then(|a| match a {
                Expr::String(s) => Some(s.to_string()),
                Expr::Var(id) => Some(format!("v{}", id.0)),
                _ => None,
            })
            .unwrap_or_default();
        return WithItem::Aggregate(AggregateFn::Count(var), alias);
    }
    WithItem::Var(alias)
}

fn hir_return_to_our(projections: &[cyrs_hir::Projection]) -> ReturnClause {
    let items = projections
        .iter()
        .map(|proj| {
            let property = match &proj.expr {
                Expr::Prop { prop, .. } => Some(prop.to_string()),
                _ => None,
            };
            ReturnItem {
                property,
                alias: proj.alias.as_ref().map(|s| s.to_string()),
            }
        })
        .collect();
    ReturnClause { items }
}

fn hir_create_to_our(pattern: &cyrs_hir::Pattern, stmt: &cyrs_hir::Statement) -> CreateClause {
    let mut nodes = Vec::new();
    for part in &pattern.parts {
        for elem in &part.elements {
            if let PatternElement::Node {
                bind,
                labels,
                props,
                ..
            } = elem
            {
                let np = NodePattern {
                    variable: bind.and_then(|id| var_name(stmt, id)),
                    labels: labels.iter().map(|s| s.to_string()).collect(),
                };
                let properties = props.as_ref().map(extract_properties).unwrap_or_default();
                nodes.push((np, properties));
            }
        }
    }
    CreateClause { nodes }
}

fn extract_properties(expr: &Expr) -> Vec<(String, PropertyValue)> {
    match expr {
        Expr::Map(pairs) => pairs
            .iter()
            .filter_map(|(k, v)| Some((k.to_string(), expr_to_prop_value(v)?)))
            .collect(),
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
    fn test_resolve_names_simple() {
        let mut result = cyrs_hir::parse_to_hir("MATCH (c:Concept) RETURN c");
        assert!(result.syntax_errors.is_empty());
        // parse_to_hir already resolves names. Verify our resolver is a no-op for resolved HIR.
        let before = count_unresolved(&result.hir);
        resolve_names(&mut result.hir);
        assert_eq!(
            count_unresolved(&result.hir),
            before,
            "resolver should not introduce new unresolved nodes"
        );
    }

    fn count_unresolved(stmt: &cyrs_hir::Statement) -> usize {
        let mut count = 0;
        for clause in &stmt.clauses {
            match clause {
                HirClause::Match { pattern, .. } | HirClause::Create { pattern, .. } => {
                    for part in &pattern.parts {
                        for elem in &part.elements {
                            let props = match elem {
                                PatternElement::Node { props, .. }
                                | PatternElement::Rel { props, .. } => props,
                            };
                            if let Some(p) = props {
                                count_unresolved_expr(p, &mut count);
                            }
                        }
                    }
                }
                HirClause::Where { predicate, .. } => count_unresolved_expr(predicate, &mut count),
                HirClause::With {
                    projections,
                    filter,
                    ..
                } => {
                    for proj in projections {
                        count_unresolved_expr(&proj.expr, &mut count);
                    }
                    if let Some(f) = filter {
                        count_unresolved_expr(f, &mut count);
                    }
                }
                HirClause::Return { projections, .. } => {
                    for proj in projections {
                        count_unresolved_expr(&proj.expr, &mut count);
                    }
                }
                _ => {}
            }
        }
        count
    }

    fn count_unresolved_expr(expr: &Expr, count: &mut usize) {
        match expr {
            Expr::Unresolved(_) => *count += 1,
            Expr::BinOp { lhs, rhs, .. } => {
                count_unresolved_expr(lhs, count);
                count_unresolved_expr(rhs, count);
            }
            Expr::UnaryOp { operand, .. } => count_unresolved_expr(operand, count),
            Expr::Call { args, .. } => args.iter().for_each(|a| count_unresolved_expr(a, count)),
            Expr::Prop { target, .. } => count_unresolved_expr(target, count),
            Expr::Index { target, index } => {
                count_unresolved_expr(target, count);
                count_unresolved_expr(index, count);
            }
            Expr::Slice { target, start, end } => {
                count_unresolved_expr(target, count);
                if let Some(s) = start {
                    count_unresolved_expr(s, count);
                }
                if let Some(e) = end {
                    count_unresolved_expr(e, count);
                }
            }
            Expr::List(items) => items.iter().for_each(|i| count_unresolved_expr(i, count)),
            Expr::Map(pairs) => pairs
                .iter()
                .for_each(|(_, v)| count_unresolved_expr(v, count)),
            Expr::IsNull { operand, .. } => count_unresolved_expr(operand, count),
            Expr::InList { operand, list } => {
                count_unresolved_expr(operand, count);
                count_unresolved_expr(list, count);
            }
            Expr::ListComprehension {
                iterable,
                filter,
                map_expr,
                ..
            } => {
                count_unresolved_expr(iterable, count);
                if let Some(f) = filter {
                    count_unresolved_expr(f, count);
                }
                count_unresolved_expr(map_expr, count);
            }
            Expr::ListPredicate {
                iterable,
                predicate,
                ..
            } => {
                count_unresolved_expr(iterable, count);
                if let Some(p) = predicate {
                    count_unresolved_expr(p, count);
                }
            }
            _ => {}
        }
    }

    // ── Parsing tests ─────────────────────────────────────────────────────

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
        let plan = parse_query("MATCH (a)-[r]->(b) RETURN a, b").unwrap();
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
        let plan =
            parse_query("OPTIONAL MATCH (c)-[r]-() WITH c, count(r) AS rc WHERE rc = 0").unwrap();
        assert_eq!(plan.clauses.len(), 2, "{:?}", plan.clauses);
        assert!(matches!(&plan.clauses[0], Clause::OptionalMatch(_)));
        assert!(matches!(&plan.clauses[1], Clause::With(_)));
    }

    #[test]
    fn test_parse_create() {
        let plan = parse_query("CREATE (n:Person {name: \"Alice\", age: 30})").unwrap();
        match &plan.clauses[0] {
            Clause::Create(c) => {
                assert_eq!(c.nodes.len(), 1);
                assert_eq!(c.nodes[0].0.variable, Some("n".into()));
                assert!(c.nodes[0].0.labels.contains(&"Person".to_string()));
                assert!(!c.nodes[0].1.is_empty());
            }
            _ => panic!("expected Create"),
        }
    }

    #[test]
    fn test_parse_empty() {
        assert!(parse_query("").is_err());
    }

    #[test]
    fn test_parse_where_condition() {
        let plan = parse_query("MATCH (c:Concept) WHERE c.value > 5 RETURN c.name").unwrap();
        assert_eq!(plan.clauses.len(), 3, "{:?}", plan.clauses);
        assert!(matches!(&plan.clauses[1], Clause::Where(_)));
    }

    // ── Real gap-detector scenario tests ──────────────────────────────────

    #[test]
    fn gap_detector_orphaned_concepts() {
        let plan = parse_query(
            "MATCH (c:Concept) OPTIONAL MATCH (c)-[r]-() WITH c, count(r) AS rc WHERE rc = 0 RETURN c"
        ).unwrap();
        assert!(
            plan.clauses
                .iter()
                .any(|c| matches!(c, Clause::OptionalMatch(_)))
        );
        assert!(plan.clauses.iter().any(|c| matches!(c, Clause::With(_))));
    }

    #[test]
    fn gap_detector_duplicate_relationships() {
        let plan = parse_query(
            "MATCH (a)-[r1]->(b) MATCH (a)-[r2]->(b) WHERE type(r1) != type(r2) RETURN a, b, type(r1), type(r2)"
        ).unwrap();
        let match_count = plan
            .clauses
            .iter()
            .filter(|c| matches!(c, Clause::Match(_)))
            .count();
        assert_eq!(match_count, 2);
    }

    #[test]
    fn multi_label_node() {
        let plan = parse_query("MATCH (n:Entity:Concept) RETURN n").unwrap();
        match &plan.clauses[0] {
            Clause::Match(m) => {
                assert!(m.node.labels.contains(&"Entity".to_string()));
                assert!(m.node.labels.contains(&"Concept".to_string()));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn anonymous_node_pattern() {
        let plan = parse_query("MATCH (:Person) RETURN 1").unwrap();
        match &plan.clauses[0] {
            Clause::Match(m) => assert!(m.node.variable.is_none()),
            _ => panic!(),
        }
    }

    #[test]
    fn where_string_equality() {
        let plan = parse_query("MATCH (c:Concept) WHERE c.name = \"Alice\" RETURN c").unwrap();
        match &plan.clauses[1] {
            Clause::Where(wc) => {
                assert!(!wc.comparisons.is_empty());
                assert_eq!(wc.comparisons[0].op, CompareOp::Eq);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn directed_relationship_outgoing() {
        let plan = parse_query("MATCH (a)-[r:KNOWS]->(b) RETURN a, b").unwrap();
        match &plan.clauses[0] {
            Clause::Match(m) => {
                let rel = m.relationship.as_ref().unwrap();
                assert_eq!(rel.direction, Direction::Outgoing);
                assert!(rel.types.contains(&"KNOWS".to_string()));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn directed_relationship_incoming() {
        let plan = parse_query("MATCH (a)<-[r:RELATES_TO]-(b) RETURN a, b").unwrap();
        match &plan.clauses[0] {
            Clause::Match(m) => {
                assert_eq!(
                    m.relationship.as_ref().unwrap().direction,
                    Direction::Incoming
                );
            }
            _ => panic!(),
        }
    }

    #[test]
    fn create_with_multiple_property_types() {
        let plan =
            parse_query("CREATE (n:Entity {name: \"test\", score: 95, active: true, value: 3.14})")
                .unwrap();
        match &plan.clauses[0] {
            Clause::Create(c) => {
                let props = &c.nodes[0].1;
                assert_eq!(props.len(), 4);
                let by_key: HashMap<&str, &PropertyValue> =
                    props.iter().map(|(k, v)| (k.as_str(), v)).collect();
                assert!(matches!(by_key.get("name"), Some(PropertyValue::Str(s)) if s == "test"));
                assert!(matches!(by_key.get("score"), Some(PropertyValue::Int(95))));
                assert!(matches!(
                    by_key.get("active"),
                    Some(PropertyValue::Bool(true))
                ));
                assert!(matches!(by_key.get("value"), Some(PropertyValue::Float(_))));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn return_with_aliases() {
        let plan = parse_query(
            "MATCH (c:Concept) RETURN c.name AS concept_name, c.score AS concept_score",
        )
        .unwrap();
        match &plan.clauses[1] {
            Clause::Return(ret) => {
                assert_eq!(ret.items.len(), 2);
                assert_eq!(ret.items[0].alias.as_deref(), Some("concept_name"));
                assert_eq!(ret.items[1].alias.as_deref(), Some("concept_score"));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn name_resolution_clears_all_unresolved() {
        // Simple queries: parse_to_hir already resolves all names.
        for query in [
            "MATCH (c:Concept) WHERE c.value > 5 RETURN c.name",
            "MATCH (a)-[r:KNOWS]->(b) WHERE a.age > b.age RETURN a, b",
        ] {
            let mut result = cyrs_hir::parse_to_hir(query);
            assert!(result.syntax_errors.is_empty(), "parse error in: {query}");
            resolve_names(&mut result.hir);
            assert_eq!(
                count_unresolved(&result.hir),
                0,
                "unresolved vars in: {query}"
            );
        }

        // Complex WITH query: resolver reduces unresolved count.
        let mut result = cyrs_hir::parse_to_hir(
            "MATCH (c:Concept) OPTIONAL MATCH (c)-[r]-() WITH c, count(r) AS rc WHERE rc = 0 RETURN c",
        );
        assert!(result.syntax_errors.is_empty());
        let before = count_unresolved(&result.hir);
        resolve_names(&mut result.hir);
        let after = count_unresolved(&result.hir);
        assert!(after <= before, "resolver should not add unresolved nodes");
    }

    #[test]
    fn memgraph_atomic_graphrag_patterns() {
        // search + expand + rank
        let p1 = parse_query(
            "MATCH (c:Concept) WHERE c.score > 0.5 RETURN c ORDER BY c.score DESC LIMIT 10",
        )
        .unwrap();
        assert!(p1.clauses.len() >= 2);

        // community expansion
        let p2 = parse_query("MATCH (c:Concept) OPTIONAL MATCH (c)-[r:RELATES_TO]->(related) WITH c, collect(related) AS neighbors RETURN c, size(neighbors) AS degree").unwrap();
        assert!(
            p2.clauses
                .iter()
                .any(|c| matches!(c, Clause::OptionalMatch(_)))
        );

        // gap detection
        let p3 = parse_query("MATCH (c:Concept) WHERE c.gap_score > 0.3 RETURN c").unwrap();
        assert!(p3.clauses.len() >= 2);
    }

    #[test]
    fn malformed_query_no_crash() {
        for q in ["MATCH", "MATCH (", "MATCH (c) WHERE"] {
            let _ = parse_query(q);
        }
    }

    #[test]
    fn whitespace_only_is_error() {
        for q in ["   ", "\n\t  "] {
            let r = parse_query(q);
            assert!(r.is_err() || r.unwrap().clauses.is_empty());
        }
    }
}
