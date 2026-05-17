// Scenario tests routed through MockGateway.
//
// Validates that the FIH protocol produces identical results when all
// primitives cross a JSON serialization boundary (simulating a real HTTP
// transport). Each scenario here mirrors its counterpart in z_scenarios.rs
// but communicates through MockGateway instead of calling GraphBlackboard
// directly.

use nexus_graph::mock_gateway::MockGateway;
use nexus_graph::{Fact, FihHash, GraphBlackboard, Intent};

/// Contradiction Detection — via MockGateway (JSON transport boundary).
///
/// Two papers make contradictory claims about GNN oversmoothing.
/// Agent-A and Agent-B each ingest a paper. Agent-C detects the
/// contradiction and submits a reconciliation hypothesis.
///
/// This is identical to scenario_contradiction_detection in z_scenarios.rs
/// except all FIH operations pass through MockGateway's JSON round-trip.
#[test]
fn scenario_contradiction_detection_via_gateway() {
    let mut gw = MockGateway::new(GraphBlackboard::new());

    // Agent-A: ingests paper claiming GNNs work fine at 50 layers
    gw.submit_fact(&Fact {
        id: FihHash("f_gnn_deep".into()),
        origin: "paper_iclr_2024".into(),
        content: serde_json::Value::String(
            "Residual GNNs maintain accuracy at 50 layers with skip connections".into(),
        ),
        creator: "agent-a".into(),
    });

    // Agent-B: ingests paper claiming GNNs oversmooth at 6 layers
    gw.submit_fact(&Fact {
        id: FihHash("f_gnn_shallow".into()),
        origin: "paper_neurips_2023".into(),
        content: serde_json::Value::String(
            "Message-passing GNNs oversmooth beyond 6 layers without normalization".into(),
        ),
        creator: "agent-b".into(),
    });

    // Agent-C: detects the contradiction, submits hypothesis
    gw.submit_intent(&Intent {
        id: FihHash("i_reconcile".into()),
        from_facts: vec!["f_gnn_deep".into(), "f_gnn_shallow".into()],
        description: "Test whether normalization technique determines oversmoothing depth".into(),
        creator: "agent-c".into(),
        worker: None,
        concluded_at: None,
    })
    .unwrap();

    // Agent-C claims, works, concludes
    gw.claim_intent("i_reconcile", "agent-c").unwrap();
    gw.conclude_intent(
        "i_reconcile",
        "Skip connections delay oversmoothing to 50+ layers; normalization alone is insufficient. Contradiction resolved.",
    ).unwrap();

    let state = gw.read_state();
    assert_eq!(state.facts.len(), 3, "2 original + 1 concluded");
    assert!(
        state.facts[2]
            .content
            .as_str()
            .unwrap_or("")
            .contains("Contradiction resolved")
    );

    println!(
        "  ✓ MockGateway: Contradiction Detection — 3 agents, JSON round-trip verified"
    );
}
