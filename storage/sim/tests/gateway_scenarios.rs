// Scenario tests for FihStorage<SimIo> using async traits.
//
// Validates that the FIH protocol produces identical results when all
// primitives go through the async trait interface. These scenarios
// mirror the sync versions in other test files but use block_on
// to drive the async FihStorage.
//
// Note: SerdeProxy<B> requires B: Blackboard (sync trait), so it
// cannot wrap FihStorage. Instead, this file tests the async trait
// interface directly on FihStorage.

use futures_executor::block_on;
use nexus_model::{
    AsyncFactCapable, AsyncIntentCapable, AsyncStorageRead, Content, Fact, FihHash, Intent,
};
use nexus_storage_sim::FihStorage;
use nexus_storage_sim::SimIo;

/// Contradiction Detection — via async traits on FihStorage.
///
/// Two papers make contradictory claims about GNN oversmoothing.
/// Agent-A and Agent-B each ingest a paper. Agent-C detects the
/// contradiction and submits a reconciliation hypothesis.
#[test]
fn scenario_contradiction_detection_via_gateway() {
    let io = SimIo::new();
    let storage = FihStorage::new(io, "test");

    // Agent-A: ingests paper claiming GNNs work fine at 50 layers
    block_on(storage.submit_fact(&Fact {
        id: FihHash::from_hex("f_gnn_deep"),
        coord: None,
        origin: "paper_iclr_2024".into(),
        content: Content::from(
            "Residual GNNs maintain accuracy at 50 layers with skip connections",
        ),
        creator: "agent-a".into(),
    }))
    .unwrap();

    // Agent-B: ingests paper claiming GNNs oversmooth at 6 layers
    block_on(storage.submit_fact(&Fact {
        id: FihHash::from_hex("f_gnn_shallow"),
        coord: None,
        origin: "paper_neurips_2023".into(),
        content: Content::from(
            "Message-passing GNNs oversmooth beyond 6 layers without normalization",
        ),
        creator: "agent-b".into(),
    }))
    .unwrap();

    // Agent-C: detects the contradiction, submits hypothesis
    block_on(storage.submit_intent(&Intent {
        id: FihHash::from_hex("i_reconcile"),
        coord: None,
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
    }))
    .unwrap();

    // Agent-C claims, works, concludes
    block_on(storage.claim_intent("i_reconcile", "agent-c")).unwrap();
    block_on(storage.conclude_intent(
        "i_reconcile",
        "Skip connections delay oversmoothing to 50+ layers; normalization alone is insufficient. Contradiction resolved.",
    )).unwrap();

    let state = block_on(storage.read_state());
    assert_eq!(state.facts.len(), 3, "2 original + 1 concluded");
    let conclusion = state
        .facts
        .iter()
        .find(|f| f.origin.starts_with("conclusion:"));
    assert!(conclusion.is_some(), "conclusion fact should exist");

    println!("  v Async: Contradiction Detection -- 3 agents verified");
}
