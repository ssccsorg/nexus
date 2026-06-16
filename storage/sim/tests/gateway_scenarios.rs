// Scenario tests routed through SerdeProxy, backed by FihStorage<SimIo>.
//
// Validates that the FIH protocol produces identical results when all
// primitives cross a JSON serialization boundary (simulating a real HTTP
// transport). Each scenario here mirrors its counterpart in z_scenarios.rs
// but communicates through SerdeProxy instead of calling HybridBlackboard
// directly.

use nexus_gateway_serde_proxy::SerdeProxy;
use nexus_model::{Content, Fact, FactCapable, FihHash, Intent, IntentCapable, StorageRead};
use nexus_storage_sim::{FihStorage, SimIo};

/// Contradiction Detection — via SerdeProxy (JSON transport boundary).
///
/// Two papers make contradictory claims about GNN oversmoothing.
/// Agent-A and Agent-B each ingest a paper. Agent-C detects the
/// contradiction and submits a reconciliation hypothesis.
///
/// This is identical to scenario_contradiction_detection in z_scenarios.rs
/// except all FIH operations pass through SerdeProxy's JSON round-trip.
#[test]
fn scenario_contradiction_detection_via_gateway() {
    let io = SimIo::new();
    let storage = FihStorage::new(io, "test");
    let gw = SerdeProxy::new(storage);

    // Agent-A: ingests paper claiming GNNs work fine at 50 layers
    gw.submit_fact(&Fact {
        id: FihHash::from_hex("f_gnn_deep"),
        origin: "paper_iclr_2024".into(),
        content: Content::from(
            "Residual GNNs maintain accuracy at 50 layers with skip connections",
        ),
        creator: "agent-a".into(),
    })
    .unwrap();

    // Agent-B: ingests paper claiming GNNs oversmooth at 6 layers
    gw.submit_fact(&Fact {
        id: FihHash::from_hex("f_gnn_shallow"),
        origin: "paper_neurips_2023".into(),
        content: Content::from(
            "Message-passing GNNs oversmooth beyond 6 layers without normalization",
        ),
        creator: "agent-b".into(),
    })
    .unwrap();

    // Agent-C: detects the contradiction, submits hypothesis
    gw.submit_intent(&Intent {
        id: FihHash::from_hex("i_reconcile"),
        from_facts: vec![
            FihHash::from_hex("f_gnn_deep"),
            FihHash::from_hex("f_gnn_shallow"),
        ],
        description: "Test whether normalization technique determines oversmoothing depth".into(),
        creator: "agent-c".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
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
    let conclusion = state
        .facts
        .iter()
        .find(|f| f.origin.starts_with("conclusion:"));
    assert!(conclusion.is_some(), "conclusion fact should exist");
    assert!(
        conclusion
            .unwrap()
            .content
            .as_str()
            .unwrap_or("")
            .contains("Contradiction resolved"),
        "conclusion fact should contain result text"
    );

    println!("  v SerdeProxy: Contradiction Detection -- 3 agents, JSON round-trip verified");
}
