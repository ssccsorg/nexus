use std::collections::HashMap;

use cyrs_hir::{self, Clause as HirClause, Expr, PatternElement};
use interface_cypher::parser::*;
use interface_cypher::{Clause, CompareOp, Direction, PropertyValue};

#[test]
fn test_resolve_names_simple() {
    let mut result = cyrs_hir::parse_to_hir("MATCH (c:Concept) RETURN c");
    assert!(result.syntax_errors.is_empty());
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

#[test]
fn gap_detector_orphaned_concepts() {
    let plan = parse_query(
        "MATCH (c:Concept) OPTIONAL MATCH (c)-[r]-() WITH c, count(r) AS rc WHERE rc = 0 RETURN c",
    )
    .unwrap();
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
    let plan =
        parse_query("MATCH (c:Concept) RETURN c.name AS concept_name, c.score AS concept_score")
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
    let p1 = parse_query(
        "MATCH (c:Concept) WHERE c.score > 0.5 RETURN c ORDER BY c.score DESC LIMIT 10",
    )
    .unwrap();
    assert!(p1.clauses.len() >= 2);

    let p2 = parse_query("MATCH (c:Concept) OPTIONAL MATCH (c)-[r:RELATES_TO]->(related) WITH c, collect(related) AS neighbors RETURN c, size(neighbors) AS degree").unwrap();
    assert!(
        p2.clauses
            .iter()
            .any(|c| matches!(c, Clause::OptionalMatch(_)))
    );

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
