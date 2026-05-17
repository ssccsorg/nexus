// Full FIH lifecycle simulation with Cypher queries.
//
// Two agents simulate a research collaboration:
//   1. Agent-A ingests facts from documents
//   2. Agent-B proposes a hypothesis (Intent) grounded in those facts
//   3. Agent-B claims, works on, and concludes the Intent
//   4. Read_state + unit assertions verify correctness (Cypher is for portability)

use nexus_graph::cypher;
use nexus_graph::{Blackboard, BlackboardError, Fact, FihHash, GraphBlackboard, Intent};

/// Helper: submit a fact with minimal boilerplate.
fn submit_fact(bb: &mut GraphBlackboard, id: &str, origin: &str, content: &str, creator: &str) {
    let fact = Fact {
        id: FihHash(id.into()),
        origin: origin.into(),
        content: content.into(),
        creator: creator.into(),
    };
    bb.submit_fact(&fact);
}

/// Helper: run a Cypher query and count results.
fn cypher_count(bb: &GraphBlackboard, query: &str) -> usize {
    let plan = cypher::Plan::from_internal(query).expect("parse failed");
    cypher::execute(bb, &plan).expect("execute failed").len()
}

#[test]
fn test_full_agent_collaboration_flow() {
    let mut bb = GraphBlackboard::new();

    // ── Phase 1: Agent-A ingests research facts ───────────────────────

    submit_fact(
        &mut bb,
        "f001",
        "arxiv_2401",
        "Graph neural networks achieve 92% accuracy on molecular property prediction",
        "agent-a",
    );
    submit_fact(
        &mut bb,
        "f002",
        "arxiv_2401",
        "Message-passing GNNs suffer from oversmoothing beyond 6 layers",
        "agent-a",
    );
    submit_fact(
        &mut bb,
        "f003",
        "nature_2023",
        "Deep learning models require 10x more data than classical ML",
        "agent-a",
    );

    // Verify: all 3 facts are stored
    let state = bb.read_state();
    assert_eq!(state.facts.len(), 3, "should have 3 facts");
    println!("  Phase 1: Agent-A ingested 3 facts");

    // Cypher: match all Fact nodes
    let count = cypher_count(&bb, "MATCH (f:Fact) RETURN f");
    assert_eq!(count, 3, "Cypher finds 3 Fact nodes");
    println!("  Phase 1: Cypher confirms 3 Fact nodes in graph");

    // ── Phase 2: Agent-B reads the blackboard and forms a hypothesis ──

    let state = bb.read_state();
    println!("  Agent-B reads: \"{}\"", state.facts[0].content);

    // Agent-B submits an Intent grounded in facts
    let intent = Intent {
        id: FihHash("i001".into()),
        from_facts: vec!["f001".into(), "f002".into()],
        description: "Test shallow GNN (3 layers) vs deep GNN (10 layers) on molecular benchmark"
            .into(),
        creator: "agent-b".into(),
        worker: None,
        concluded_at: None,
    };
    bb.submit_intent(&intent).expect("intent should be valid");

    // Verify: intent is in read_state
    let state = bb.read_state();
    assert_eq!(state.intents.len(), 1);
    assert_eq!(state.intents[0].description, intent.description);

    // Cypher: verify both Fact and Intent nodes exist
    let fact_count = cypher_count(&bb, "MATCH (f:Fact) RETURN f");
    let intent_count = cypher_count(&bb, "MATCH (i:Intent) RETURN i");
    assert_eq!(fact_count, 3, "facts unchanged");
    assert_eq!(intent_count, 1, "1 intent submitted");
    println!(
        "  Phase 2: Agent-B submitted Intent — Cypher: {} facts, {} intents",
        fact_count, intent_count
    );

    // ── Phase 3: Agent-B claims and works on the Intent ───────────────

    bb.claim_intent("i001", "agent-b")
        .expect("claim should succeed");

    // Cypher: the intent node still exists
    let intent_count = cypher_count(&bb, "MATCH (i:Intent) RETURN i");
    assert_eq!(intent_count, 1);
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

    let result = "Shallow GNN (3 layers) achieves 94% accuracy vs 89% for deep (10 layers)";
    let (new_fact, follow_ups) = bb
        .conclude_intent("i001", &result.into())
        .expect("conclude should succeed");

    assert_eq!(new_fact.content, result);
    assert!(!follow_ups.is_empty(), "should generate follow-up");
    println!(
        "  Phase 4: Concluded → new Fact + {} follow-up Intent(s)",
        follow_ups.len()
    );

    // Agent-B reviews and submits the follow-up Intent
    for next_intent in &follow_ups {
        bb.submit_intent(next_intent)
            .expect("follow-up intent should be valid");
    }
    println!(
        "  Phase 4: Agent-B submitted {} follow-up Intent(s)",
        follow_ups.len()
    );

    // ── Phase 5: Verify final state ───────────────────────────────────

    let state = bb.read_state();
    assert_eq!(state.facts.len(), 4, "3 original + 1 concluded = 4 facts");
    assert_eq!(
        state.intents.len(),
        2,
        "1 original + 1 follow-up = 2 intents"
    );

    // Cypher: final node counts
    let fact_count = cypher_count(&bb, "MATCH (f:Fact) RETURN f");
    let intent_count = cypher_count(&bb, "MATCH (i:Intent) RETURN i");
    assert_eq!(fact_count, 4);
    assert_eq!(intent_count, 2);

    println!(
        "  Phase 5: Final state — Cypher: {} facts, {} intents",
        fact_count, intent_count
    );
    println!();
    println!("  ✓ Full FIH lifecycle + Cypher queries work end-to-end");
    println!("  ✓ 3 agents (A, B, C) interacting through Blackboard alone");
    println!("  ✓ No direct agent-to-agent communication — all via FIH");
}
