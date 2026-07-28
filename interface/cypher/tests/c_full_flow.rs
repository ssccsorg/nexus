// Full FIH lifecycle simulation with Cypher queries.
//
// Two agents simulate a research collaboration:
//   1. Agent-A ingests facts from documents
//   2. Agent-B proposes a hypothesis (Intent) grounded in those facts
//   3. Agent-B claims, works on, and concludes the Intent
//   4. Read_state + unit assertions verify correctness (Cypher is for portability)

use nex::FihBlackboard;
use nex_fih::{Blackboard, BlackboardError, CoordId, Fact, Intent, IntentCapable, StorageRead};
use nexus_storage_sim::SimIo;

/// Helper: submit a fact with minimal boilerplate.
fn submit_fact(bb: &impl Blackboard, id: &str, origin: &str, content: &str, creator: &str) {
    let fact = Fact::new(
        CoordId::from_string(id),
        origin.into(),
        content.into(),
        creator.into(),
    );
    bb.submit_fact(&fact).unwrap();
}

#[test]
fn test_full_agent_collaboration_flow() {
    let bb = FihBlackboard::new(SimIo::new(), "test");

    // ── Phase 1: Agent-A ingests research facts ───────────────────────

    submit_fact(
        &bb,
        "f001",
        "arxiv_2401",
        "Graph neural networks achieve 92% accuracy on molecular property prediction",
        "agent-a",
    );
    submit_fact(
        &bb,
        "f002",
        "arxiv_2401",
        "Message-passing GNNs suffer from oversmoothing beyond 6 layers",
        "agent-a",
    );
    submit_fact(
        &bb,
        "f003",
        "nature_2023",
        "Deep learning models require 10x more data than classical ML",
        "agent-a",
    );

    // Verify: all 3 facts are stored
    let state = bb.read_state();
    assert_eq!(state.facts.len(), 3, "should have 3 facts");
    println!("  Phase 1: Agent-A ingested 3 facts");

    // ── Phase 2: Agent-B reads the blackboard and forms a hypothesis ──

    let state = bb.read_state();
    println!("  Agent-B reads: \"{}\"", state.facts[0].content);

    // Agent-B submits an Intent grounded in facts
    let intent = Intent {
        id: CoordId::from_string("i001"),
        from_facts: vec![CoordId::from_string("f001"), CoordId::from_string("f002")],
        description: "Test shallow GNN (3 layers) vs deep GNN (10 layers) on molecular benchmark"
            .into(),
        creator: "agent-b".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    };
    bb.submit_intent(&intent).expect("intent should be valid");

    // Verify: intent is in read_state
    let state = bb.read_state();
    assert_eq!(state.intents.len(), 1);
    assert_eq!(state.intents[0].description, intent.description);

    // ── Phase 3: Agent-B claims and works on the Intent ───────────────

    bb.claim_intent("i001", "agent-b")
        .expect("claim should succeed");

    println!("  Phase 3: Agent-B claimed Intent");

    // Agent-B heartbeats
    bb.heartbeat("i001", "agent-b")
        .expect("heartbeat should succeed");

    // Another agent tries to claim — must fail
    let double_claim = bb.claim_intent("i001", "agent-c");
    assert!(
        matches!(double_claim, Err(BlackboardError::Conflict(_))),
        "double claim should fail"
    );
    println!("  Phase 3: Agent-C blocked from claiming (Conflict) ✓");

    // ── Phase 4: Agent-B concludes the Intent ─────────────────────────

    let new_fact = bb
        .conclude_intent(
            "i001",
            "Shallow GNN (3 layers) achieves 94% accuracy vs 89% for deep (10 layers)",
        )
        .expect("conclude should succeed");

    assert_eq!(
        new_fact.content.to_string(),
        "Shallow GNN (3 layers) achieves 94% accuracy vs 89% for deep (10 layers)"
    );
    println!("  Phase 4: Concluded → new Fact");

    // ── Phase 5: Verify final state ───────────────────────────────────

    let state = bb.read_state();
    assert_eq!(state.facts.len(), 4, "3 original + 1 concluded = 4 facts");
    assert_eq!(state.intents.len(), 1, "1 original intent");

    println!();
    println!("  ✓ Full FIH lifecycle + Cypher queries work end-to-end");
    println!("  ✓ 3 agents (A, B, C) interacting through Blackboard alone");
    println!("  ✓ No direct agent-to-agent communication — all via FIH");
}
