// Cold query routing bridge integration tests.
//
// Tests Plan→ColdQuery translation for both External (cyrs_plan) and
// Internal (PlanIR) plan variants, plus execute_with_cold hot/cold routing.

use nexus_graph::query::cypher::{Plan, execute_with_cold};
use nexus_graph::{EdgeWeight, NodeWeight};

// ── External Plan → ColdQuery ───────────────────────────────────────────────

#[test]
fn external_source_fact() {
    let plan = Plan::from_cyrs("MATCH (f:Fact) RETURN f").unwrap();
    let cold = plan.to_cold_query();
    assert!(cold.is_some());
    assert_eq!(cold.unwrap().label, "Fact");
}

#[test]
fn external_source_intent() {
    let plan = Plan::from_cyrs("MATCH (i:Intent) RETURN i").unwrap();
    let cold = plan.to_cold_query();
    assert!(cold.is_some());
    assert_eq!(cold.unwrap().label, "Intent");
}

#[test]
fn external_source_hint() {
    let plan = Plan::from_cyrs("MATCH (h:Hint) RETURN h").unwrap();
    let cold = plan.to_cold_query();
    assert!(cold.is_some());
    assert_eq!(cold.unwrap().label, "Hint");
}

#[test]
fn external_non_fih_label_returns_none() {
    let plan = Plan::from_cyrs("MATCH (n:Person) RETURN n").unwrap();
    assert!(plan.to_cold_query().is_none());
}

#[test]
fn external_with_expand_returns_none() {
    let plan = Plan::from_cyrs("MATCH (f:Fact)-[:drives]->(i:Intent) RETURN f").unwrap();
    assert!(plan.to_cold_query().is_none());
}

#[test]
fn external_source_filter_eq() {
    let plan = Plan::from_cyrs("MATCH (f:Fact) WHERE f.origin = 'test' RETURN f").unwrap();
    let cold = plan.to_cold_query();
    assert!(cold.is_some());
    let cq = cold.unwrap();
    assert_eq!(cq.label, "Fact");
    assert!(!cq.filters.is_empty());
    let eq = cq.filters.iter().find(|f| f.op == "Eq").unwrap();
    assert_eq!(eq.value, serde_json::json!("test"));
}

#[test]
fn external_source_filter_gt() {
    let plan = Plan::from_cyrs("MATCH (f:Fact) WHERE f.priority > 5 RETURN f").unwrap();
    let cold = plan.to_cold_query();
    assert!(cold.is_some());
    let cq = cold.unwrap();
    let gt = cq.filters.iter().find(|f| f.op == "Gt").unwrap();
    assert_eq!(gt.value, serde_json::json!(5));
}

#[test]
fn external_source_project_limit() {
    let plan = Plan::from_cyrs("MATCH (f:Fact) RETURN f.fact_id LIMIT 10").unwrap();
    let cold = plan.to_cold_query();
    assert!(cold.is_some());
    let cq = cold.unwrap();
    assert_eq!(cq.label, "Fact");
    assert_eq!(cq.limit, Some(10));
}

// ── Internal Plan (PlanIR) → ColdQuery ──────────────────────────────────────

#[test]
fn internal_simple_match_fact() {
    let plan = Plan::from_internal("MATCH (f:Fact) RETURN f.fact_id").unwrap();
    let cold = plan.to_cold_query();
    assert!(cold.is_some());
    assert_eq!(cold.unwrap().label, "Fact");
}

#[test]
fn internal_match_with_relationship_returns_none() {
    let plan = Plan::from_internal("MATCH (f:Fact)-[:drives]->(i:Intent) RETURN f").unwrap();
    assert!(plan.to_cold_query().is_none());
}

#[test]
fn internal_non_fih_label_returns_none() {
    let plan = Plan::from_internal("MATCH (n:Person) RETURN n").unwrap();
    assert!(plan.to_cold_query().is_none());
}

#[test]
fn internal_optional_match_returns_none() {
    let plan = Plan::from_internal("OPTIONAL MATCH (f:Fact) RETURN f.fact_id").unwrap();
    assert!(plan.to_cold_query().is_none());
}

// ── execute_with_cold: hot/cold routing ────────────────────────────────────

#[test]
fn execute_with_cold_non_eligible_falls_back_to_hot() {
    // A plan with a relationship (Expand op) is not cold-eligible.
    // execute_with_cold should fall back to hot petgraph execution.
    let g = petgraph::Graph::<NodeWeight, EdgeWeight>::new();
    let plan = Plan::from_cyrs("MATCH (f:Fact)-[:drives]->(i:Intent) RETURN f").unwrap();
    let cold = nexus_model::NullStorage;
    let result = execute_with_cold(&g, &cold, &plan);
    assert!(result.is_ok(), "expected hot fallback, got: {:?}", result);
}

#[test]
fn execute_with_cold_eligible_errors_on_null_storage() {
    // A cold-eligible plan with NullStorage should attempt cold routing.
    // NullStorage.query_plan returns an error, so this should fail.
    let g = petgraph::Graph::<NodeWeight, EdgeWeight>::new();
    let plan = Plan::from_cyrs("MATCH (f:Fact) RETURN f").unwrap();
    let cold = nexus_model::NullStorage;
    let result = execute_with_cold(&g, &cold, &plan);
    assert!(
        result.is_err(),
        "expected cold routing error, got: {:?}",
        result
    );
}

#[test]
fn execute_with_cold_eligible_with_populated_graph() {
    // Graph has data, but cold-eligible plan routes to NullStorage → error.
    // This confirms the cold path was attempted, not silently falling back to hot.
    let mut graph = petgraph::Graph::<NodeWeight, EdgeWeight>::new();
    graph.add_node(NodeWeight {
        name: "f_test".into(),
        label: "Fact".into(),
        properties: std::collections::HashMap::from([(
            "origin".into(),
            serde_json::Value::String("test-source".into()),
        )]),
    });
    let plan = Plan::from_internal("MATCH (f:Fact) RETURN f").unwrap();
    let cold = nexus_model::NullStorage;
    let result = execute_with_cold(&graph, &cold, &plan);
    assert!(
        result.is_err(),
        "expected cold routing error (cold path attempted), got: {:?}",
        result
    );
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("CypherCapable") || err_msg.contains("not implemented"),
        "unexpected error message: {}",
        err_msg
    );
}
