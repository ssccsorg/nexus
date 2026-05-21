// Rust privileged agent: direct Blackboard trait + GraphRead consumer.
//
// Demonstrates how an internal (privileged) agent imports the nexus-graph
// crate directly to access the Blackboard trait and execute Cypher queries
// through GraphRead. This is the pattern for agents that run in-process
// (dispatcher, gap-detector, verifier).
//
// External agents use the HTTP gateway instead (see tests/consumers/).
//
// Usage:
//   cd tests/agents && cargo run

use nexus_graph::cypher;
use nexus_graph::{Blackboard, DefaultBlackboard, Fact, FihHash, Intent};

fn main() {
    println!("=== Rust Privileged Agent: Direct Blackboard Access ===\n");

    let mut bb = DefaultBlackboard::new();

    // ── Phase 1: Submit facts ────────────────────────────────────────

    println!("1. Submitting facts...");
    let f1 = bb.submit_fact(&Fact {
        id: FihHash::new(&["gnn-accuracy"], "fact"),
        origin: "arxiv_2401".into(),
        content: serde_json::json!({
            "model": "GNN",
            "accuracy": 0.92,
            "dataset": "ogbn-arxiv",
            "layers": 3
        }),
        creator: "agent-a".into(),
    }).unwrap();
    let f2 = bb.submit_fact(&Fact {
        id: FihHash::new(&["gnn-oversmoothing"], "fact"),
        origin: "neurips_2023".into(),
        content: serde_json::json!({
            "finding": "Message-passing GNNs oversmooth beyond 6 layers",
            "threshold": 6
        }),
        creator: "agent-b".into(),
    }).unwrap();
    println!("   Fact 1: {}", f1);
    println!("   Fact 2: {}", f2);

    // ── Phase 2: Submit intent ───────────────────────────────────────

    println!("\n2. Submitting intent...");
    let intent_id = bb
        .submit_intent(&Intent {
            id: FihHash::new(&["test-hypothesis"], "intent"),
            from_facts: vec![f1.0.clone(), f2.0.clone()],
            description: "Test shallow (3-layer) vs deep GNN on molecular benchmark".into(),
            creator: "agent-c".into(),
            worker: None,
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            concluded_at: None,
        })
        .expect("intent grounded");
    println!("   Intent: {}", intent_id);

    // ── Phase 3: Claim → Heartbeat → Conclude ───────────────────────

    println!("\n3. Claiming...");
    bb.claim_intent(&intent_id.0, "agent-c").unwrap();
    println!("   Claimed by agent-c");

    println!("\n4. Heartbeat...");
    bb.heartbeat(&intent_id.0, "agent-c").unwrap();
    println!("   OK");

    println!("\n5. Concluding...");
    let new_fact = bb
        .conclude_intent(
            &intent_id.0,
            &serde_json::json!({
                "result": "Shallow GNN (3 layers): 94% accuracy. Deep GNN (10 layers): 89%.",
                "winner": "shallow",
                "delta": 0.05
            }),
        )
        .unwrap();
    println!("   New fact: {} = {}", new_fact.id, new_fact.content);

    // ── Phase 4: Privileged Cypher query ─────────────────────────────

    println!("\n6. Privileged: Cypher MATCH query...");
    let plan = cypher::Plan::from_internal("MATCH (f:Fact) RETURN f").unwrap();
    let rows = cypher::execute(&bb, &plan).unwrap();
    println!("   Cypher returned {} rows (all Fact nodes)", rows.len());

    let plan2 = cypher::Plan::from_internal("MATCH (i:Intent) RETURN i").unwrap();
    let rows2 = cypher::execute(&bb, &plan2).unwrap();
    println!("   Cypher returned {} rows (all Intent nodes)", rows2.len());

    // ── Final state ──────────────────────────────────────────────────

    let state = bb.read_state();
    println!("\n=== Final Board State ===");
    println!("   Facts:   {}", state.facts.len());
    println!("   Intents: {}", state.intents.len());
    println!("   Hints:   {}", state.hints.len());
    println!("\n   (External HTTP agent would see the same BoardState via GET /state)");
    println!("   (Privileged agent has GraphRead + Cypher executor, external agents do not)");

    println!("\nRust privileged agent: direct trait + Cypher access complete");
}
