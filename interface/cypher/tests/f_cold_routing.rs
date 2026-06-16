// Cold query routing bridge integration tests.
//
// Tests Plan-ColdQuery translation for both External (cyrs_plan) and
// Internal (PlanIR) plan variants, plus execute_with_cold hot/cold routing.

use interface_cypher::{Plan, execute_with_cold};
use interface_query::ColdQuery;
use nexus_storage_petgraph::{EdgeWeight, NodeWeight};

fn assert_label(cq: &ColdQuery, expected: &str) {
    assert_eq!(cq.label, expected);
}

fn assert_filters_nonempty(cq: &ColdQuery) {
    assert!(!cq.filters.is_empty());
}

// -- External Plan to ColdQuery

#[test]
fn external_source_fact() {
    let plan = Plan::from_cyrs("MATCH (f:Fact) RETURN f").unwrap();
    let cold = plan.to_cold_query();
    assert!(cold.is_some());
    assert_label(&cold.unwrap(), "Fact");
}

#[test]
fn external_source_intent() {
    let plan = Plan::from_cyrs("MATCH (i:Intent) RETURN i").unwrap();
    let cold = plan.to_cold_query();
    assert!(cold.is_some());
    assert_label(&cold.unwrap(), "Intent");
}

#[test]
fn external_source_hint() {
    let plan = Plan::from_cyrs("MATCH (h:Hint) RETURN h").unwrap();
    let cold = plan.to_cold_query();
    assert!(cold.is_some());
    assert_label(&cold.unwrap(), "Hint");
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
    assert_label(&cq, "Fact");
    assert_filters_nonempty(&cq);
    let eq_filter = cq.filters.iter().find(|f| f.op == "Eq").unwrap();
    assert_eq!(eq_filter.value, serde_json::json!("test"));
}

#[test]
fn external_source_filter_gt() {
    let plan = Plan::from_cyrs("MATCH (f:Fact) WHERE f.priority > 5 RETURN f").unwrap();
    let cold = plan.to_cold_query();
    assert!(cold.is_some());
    let cq = cold.unwrap();
    let gt_filter = cq.filters.iter().find(|f| f.op == "Gt").unwrap();
    assert_eq!(gt_filter.value, serde_json::json!(5));
}

#[test]
fn external_source_project_limit() {
    let plan = Plan::from_cyrs("MATCH (f:Fact) RETURN f.fact_id LIMIT 10").unwrap();
    let cold = plan.to_cold_query();
    assert!(cold.is_some());
    let cq = cold.unwrap();
    assert_label(&cq, "Fact");
    assert_eq!(cq.limit, Some(10));
    assert_eq!(cq.projections, vec!["fact_id".to_string()]);
}

// -- Internal Plan (PlanIR) to ColdQuery

#[test]
fn internal_simple_match_fact() {
    let plan = Plan::from_internal("MATCH (f:Fact) RETURN f.fact_id").unwrap();
    let cold = plan.to_cold_query();
    assert!(cold.is_some());
    assert_label(&cold.unwrap(), "Fact");
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

// -- Internal Plan to ColdQuery: string WHERE (was broken before fix)

#[test]
fn internal_match_where_string_eq() {
    let plan =
        Plan::from_internal("MATCH (f:Fact) WHERE f.origin = 'test-source' RETURN f.fact_id")
            .unwrap();
    let cold = plan.to_cold_query();
    assert!(cold.is_some(), "string WHERE should not fail");
    let cq = cold.unwrap();
    assert_filters_nonempty(&cq);
}

// -- execute_with_cold: hot/cold routing

#[test]
fn execute_with_cold_non_eligible_falls_back_to_hot() {
    let g = petgraph::Graph::<NodeWeight, EdgeWeight>::new();
    let plan = Plan::from_cyrs("MATCH (f:Fact)-[:drives]->(i:Intent) RETURN f").unwrap();
    let cold = nexus_model::NullStorage;
    let result = execute_with_cold(&g, &cold, &plan);
    assert!(result.is_ok(), "expected hot fallback, got: {:?}", result);
}

#[test]
fn execute_with_cold_eligible_errors_on_null_storage() {
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
    let mut graph = petgraph::Graph::<NodeWeight, EdgeWeight>::new();
    graph.add_node(NodeWeight {
        name: "f_test".into(),
        label: "Fact".into(),
        properties: std::collections::HashMap::from([("origin".into(), "\"test-source\"".into())]),
    });
    let plan = Plan::from_internal("MATCH (f:Fact) RETURN f").unwrap();
    let cold = nexus_model::NullStorage;
    let result = execute_with_cold(&graph, &cold, &plan);
    assert!(
        result.is_err(),
        "expected cold routing error (cold path attempted), got: {:?}",
        result
    );
}
