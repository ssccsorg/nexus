// Multi-agent research scenarios solving actual problems through the Blackboard.
//
// Each scenario: a group of agents use FIH to collaboratively solve a concrete
// research problem. No agent talks directly to another — all via Blackboard.

use nexus_graph::cypher;
use nexus_graph::{Blackboard, Fact, FihHash, GraphBlackboard, Intent, Hint};

// ── Scenario 1: Contradiction Detection ───────────────────────────────────
//
// Two papers make contradictory claims about GNN oversmoothing.
// Agent-A and Agent-B each ingest a paper. Agent-C detects the contradiction
// and submits a reconciliation hypothesis.

#[test]
fn scenario_contradiction_detection() {
    let mut bb = GraphBlackboard::new();

    // Agent-A: ingests paper claiming GNNs work fine at 50 layers
    bb.submit_fact(&Fact {
        id: FihHash("f_gnn_deep".into()),
        origin: "paper_iclr_2024".into(),
        content: "Residual GNNs maintain accuracy at 50 layers with skip connections".into(),
        creator: "agent-a".into(),
    });

    // Agent-B: ingests paper claiming GNNs oversmooth at 6 layers
    bb.submit_fact(&Fact {
        id: FihHash("f_gnn_shallow".into()),
        origin: "paper_neurips_2023".into(),
        content: "Message-passing GNNs oversmooth beyond 6 layers without normalization".into(),
        creator: "agent-b".into(),
    });

    // Agent-C: detects the contradiction, submits hypothesis
    bb.submit_intent(&Intent {
        id: FihHash("i_reconcile".into()),
        from_facts: vec!["f_gnn_deep".into(), "f_gnn_shallow".into()],
        description: "Test whether normalization technique determines oversmoothing depth".into(),
        creator: "agent-c".into(),
        worker: None,
        concluded_at: None,
    }).unwrap();

    // Agent-C claims, works, concludes
    bb.claim_intent("i_reconcile", "agent-c").unwrap();
    bb.conclude_intent("i_reconcile", "Skip connections delay oversmoothing to 50+ layers; normalization alone is insufficient. Contradiction resolved.").unwrap();

    let state = bb.read_state();
    assert_eq!(state.facts.len(), 3, "2 original + 1 concluded");
    assert!(state.facts[2].content.contains("Contradiction resolved"));

    // Cypher verification
    let plan = cypher::Plan::from_internal("MATCH (f:Fact) RETURN f").unwrap();
    let rows = cypher::execute(&bb, &plan).unwrap();
    assert_eq!(rows.len(), 3);

    println!("  ✓ Contradiction Detection: 3 agents, contradiction resolved via FIH");
}

// ── Scenario 2: Peer Review Pipeline ──────────────────────────────────────
//
// Agent-A submits a research hypothesis. Agent-B and Agent-C act as reviewers.
// Agent-D acts as editor. Each leaves Hints (review comments).
// Editor concludes based on reviewer hints.

#[test]
fn scenario_peer_review() {
    let mut bb = GraphBlackboard::new();

    // Phase 1: Agent-A submits hypothesis as Intent
    let hypothesis = Intent {
        id: FihHash("i_hypothesis".into()),
        from_facts: vec!["f_background".into()],
        description: "Quantum error correction with surface codes achieves fault tolerance at 0.1% error rate".into(),
        creator: "agent-a".into(),
        worker: None,
        concluded_at: None,
    };
    // Need a grounding fact first
    bb.submit_fact(&Fact {
        id: FihHash("f_background".into()),
        origin: "background".into(),
        content: "Surface codes are the leading QEC candidate".into(),
        creator: "system".into(),
    });
    bb.submit_intent(&hypothesis).unwrap();

    // Phase 2: Reviewer agents submit Hints (review comments)
    bb.submit_hint(&Hint {
        id: FihHash("h_reviewer1".into()),
        content: "The 0.1% threshold seems optimistic — reference recent experiments".into(),
        creator: "agent-b".into(),
    });
    bb.submit_hint(&Hint {
        id: FihHash("h_reviewer2".into()),
        content: "Need to specify distance-3 vs distance-5 surface code constraints".into(),
        creator: "agent-c".into(),
    });

    // Phase 3: Editor reads hints and concludes
    bb.claim_intent("i_hypothesis", "agent-d").unwrap();

    let state = bb.read_state();
    assert_eq!(state.hints.len(), 2, "2 review hints submitted");
    assert!(state.hints.iter().any(|h| h.creator == "agent-b"), "hint from agent-b");

    // Editor concludes incorporating feedback
    let (result, _) = bb.conclude_intent(
        "i_hypothesis",
        "Hypothesis accepted with revisions: surface code QEC at distance-3 achieves 0.1% threshold (confirmed). Distance-5 requires 0.05%.",
    ).unwrap();
    assert!(result.content.contains("accepted with revisions"));

    // Verify via Cypher
    let hint_count = {
        let p = cypher::Plan::from_internal("MATCH (h:Hint) RETURN h").unwrap();
        cypher::execute(&bb, &p).unwrap().len()
    };
    assert_eq!(hint_count, 2, "Cypher finds 2 hints");

    println!("  ✓ Peer Review: author + 2 reviewers + editor, hints guide conclusion");
}

// ── Scenario 3: Knowledge Synthesis (Jigsaw Puzzle) ───────────────────────
//
// Three agents each hold one piece of a larger picture.
// Agent-D synthesizes by reading all facts and submitting a unified theory.
// This tests that read_state() returns complete cross-agent knowledge.

#[test]
fn scenario_knowledge_synthesis() {
    let mut bb = GraphBlackboard::new();

    // Three agents each submit partial observations
    let pieces = [
        ("f_piece_1", "agent-alpha",
         "Battery cell temperature rises 15°C under 2C discharge rate"),
        ("f_piece_2", "agent-beta",
         "Electrolyte viscosity doubles below -10°C ambient temperature"),
        ("f_piece_3", "agent-gamma",
         "Anode lithium plating occurs at charge rates above 1C below 0°C"),
    ];
    for (id, creator, content) in &pieces {
        bb.submit_fact(&Fact {
            id: FihHash(id.to_string()),
            origin: "experiment".into(),
            content: content.to_string(),
            creator: creator.to_string(),
        });
    }

    // Agent-D reads all and synthesizes
    let state = bb.read_state();
    assert_eq!(state.facts.len(), 3);

    // Agent-D recognizes the pattern: cold + high rate = lithium plating
    bb.submit_fact(&Fact {
        id: FihHash("f_synthesis".into()),
        origin: "synthesis".into(),
        content: "SYNTHESIS: Low temperature (-10°C) increases electrolyte viscosity, reducing ion mobility. High discharge rate (2C) generates heat (15°C rise). Combined high charge rate (>1C) below 0°C causes anode lithium plating. Solution: preheat battery to 10°C before fast charging in cold environments.".into(),
        creator: "agent-delta".into(),
    });

    // Verify synthesis
    let state = bb.read_state();
    assert_eq!(state.facts.len(), 4, "3 pieces + 1 synthesis");
    let synthesis = &state.facts[3];
    assert!(synthesis.content.contains("SYNTHESIS"), "synthesis marker present");
    assert!(synthesis.content.contains("preheat"), "actionable solution proposed");

    // Agent-D's synthesis should be propositional — others can build on it
    bb.submit_intent(&Intent {
        id: FihHash("i_validate_synthesis".into()),
        from_facts: vec!["f_synthesis".into()],
        description: "Experimental validation: test preheat to 10°C before 2C charging at -10°C ambient".into(),
        creator: "agent-epsilon".into(),
        worker: None,
        concluded_at: None,
    }).unwrap();

    let state = bb.read_state();
    assert_eq!(state.intents.len(), 1, "validation intent submitted");

    // Cypher: all pieces accessible
    let count = {
        let p = cypher::Plan::from_internal("MATCH (f:Fact) RETURN f").unwrap();
        cypher::execute(&bb, &p).unwrap().len()
    };
    assert_eq!(count, 4, "Cypher confirms 4 facts");
    assert_eq!(state.facts.len(), count, "read_state matches Cypher");

    println!("  ✓ Knowledge Synthesis: 4 agents complete a jigsaw puzzle via Blackboard");
    println!("  ✓ Partial observations → unified theory → actionable hypothesis");
}

// ── Scenario 4: Emergency Response Coordination ───────────────────────────
//
// Simulates a crisis: multiple agents detect different signals,
// a coordinator agent prioritizes, and response agents execute.
// Tests concurrent intent claim and lifecycle under pressure.

#[test]
fn scenario_emergency_response() {
    let mut bb = GraphBlackboard::new();

    // Sensor agents detect anomalies
    let alerts = [
        ("f_alarm_smoke", "sensor-alpha", "Smoke detected in sector 7, visibility 2m"),
        ("f_alarm_temp", "sensor-beta", "Temperature spike 85°C in sector 7, rising 2°/min"),
        ("f_alarm_power", "sensor-gamma", "Power line to sector 7 shows 40% voltage drop"),
    ];
    for (id, creator, content) in &alerts {
        bb.submit_fact(&Fact {
            id: FihHash(id.to_string()),
            origin: "sensor".into(),
            content: content.to_string(),
            creator: creator.to_string(),
        });
    }

    // Coordinator submits prioritized response plan as Intent
    bb.submit_intent(&Intent {
        id: FihHash("i_respond_fire".into()),
        from_facts: vec![
            "f_alarm_smoke".into(),
            "f_alarm_temp".into(),
            "f_alarm_power".into(),
        ],
        description: "FIRE_RESPONSE: Evacuate sector 7, activate fire suppression, isolate power".into(),
        creator: "coordinator".into(),
        worker: None,
        concluded_at: None,
    }).unwrap();

    // Response agents compete to claim the response intent
    let responders = ["responder-alpha", "responder-beta", "responder-gamma"];
    let mut claimed_by = None;
    for agent in &responders {
        match bb.claim_intent("i_respond_fire", agent) {
            Ok(()) => {
                claimed_by = Some(agent);
                break;
            }
            Err(_) => continue,
        }
    }
    let champion = claimed_by.expect("some responder must claim");
    println!("  Responder {champion} claimed fire response");

    // Other responders should fail
    for agent in &responders {
        if agent == champion {
            continue;
        }
        let result = bb.claim_intent("i_respond_fire", agent);
        assert!(result.is_err(), "{agent} should not claim claimed intent");
    }

    // Champion reports progress
    bb.heartbeat("i_respond_fire", champion).unwrap();

    // Champion concludes
    let (outcome, _) = bb
        .conclude_intent("i_respond_fire", "Sector 7 evacuated, fire suppressed in 3min, power isolated. No casualties.")
        .unwrap();
    assert!(outcome.content.contains("evacuated"));

    // Final state
    let state = bb.read_state();
    assert_eq!(state.facts.len(), 4, "3 alerts + 1 outcome");
    assert_eq!(state.intents.len(), 1, "response intent (not yet followed up)");

    println!("  ✓ Emergency Response: 3 sensors + coordinator + 3 responders = 7 agents");
    println!("  ✓ Competitive claim: only one responder wins, others rejected");
    println!("  ✓ Full lifecycle: alert → plan → claim → heartbeat → conclude");
}
