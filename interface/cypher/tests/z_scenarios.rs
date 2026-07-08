// Multi-agent research scenarios solving actual problems through the Blackboard.
//
// Each scenario: a group of agents use FIH to collaboratively solve a concrete
// research problem. No agent talks directly to another — all via Blackboard.

use interface_cypher as cypher;
use nexus_model::{
    Fact, FactCapable, FihHash, Hint, HintCapable, Intent, IntentCapable, StorageRead,
};
use nexus_storage_composite::HybridBlackboard;

// ── Scenario 1: Contradiction Detection ───────────────────────────────────
//
// Two papers make contradictory claims about GNN oversmoothing.
// Agent-A and Agent-B each ingest a paper. Agent-C detects the contradiction
// and submits a reconciliation hypothesis.

#[test]
fn scenario_contradiction_detection() {
    let bb = HybridBlackboard::new();

    // Agent-A: ingests paper claiming GNNs work fine at 50 layers
    bb.submit_fact(&Fact {
        id: FihHash::from_hex("f_gnn_deep"),
        origin: "paper_iclr_2024".into(),
        content: "Residual GNNs maintain accuracy at 50 layers with skip connections".into(),
        creator: "agent-a".into(),
    })
    .unwrap();

    // Agent-B: ingests paper claiming GNNs oversmooth at 6 layers
    bb.submit_fact(&Fact {
        id: FihHash::from_hex("f_gnn_shallow"),
        origin: "paper_neurips_2023".into(),
        content: "Message-passing GNNs oversmooth beyond 6 layers without normalization".into(),
        creator: "agent-b".into(),
    })
    .unwrap();

    // Agent-C: detects the contradiction, submits hypothesis
    bb.submit_intent(&Intent {
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
    bb.claim_intent("i_reconcile", "agent-c").unwrap();
    bb.conclude_intent("i_reconcile", "Skip connections delay oversmoothing to 50+ layers; normalization alone is insufficient. Contradiction resolved.").unwrap();

    let state = bb.read_state();
    assert_eq!(state.facts.len(), 3, "2 original + 1 concluded");
    assert!(
        state.facts[2]
            .content
            .as_str()
            .unwrap_or("")
            .contains("Contradiction resolved")
    );

    // Cypher verification
    let rows = bb.with_graph(|g| {
        let plan = cypher::Plan::from_internal("MATCH (f:Fact) RETURN f").unwrap();
        cypher::execute(g, &plan).unwrap()
    });
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
    let bb = HybridBlackboard::new();

    // Phase 1: Agent-A submits hypothesis as Intent
    let hypothesis = Intent {
        id: FihHash::from_hex("i_hypothesis"),
        from_facts: vec![FihHash::from_hex("f_background")],
        description: "Quantum error correction with surface codes achieves fault tolerance at 0.1% error rate".into(),
        creator: "agent-a".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    };
    // Need a grounding fact first
    bb.submit_fact(&Fact {
        id: FihHash::from_hex("f_background"),
        origin: "background".into(),
        content: "Surface codes are the leading QEC candidate".into(),
        creator: "system".into(),
    })
    .unwrap();
    bb.submit_intent(&hypothesis).unwrap();

    // Phase 2: Reviewer agents submit Hints (review comments)
    bb.submit_hint(&Hint {
        id: FihHash::from_hex("h_reviewer1"),
        content: "The 0.1% threshold seems optimistic — reference recent experiments".into(),
        creator: "agent-b".into(),
    })
    .unwrap();
    bb.submit_hint(&Hint {
        id: FihHash::from_hex("h_reviewer2"),
        content: "Need to specify distance-3 vs distance-5 surface code constraints".into(),
        creator: "agent-c".into(),
    })
    .unwrap();

    // Phase 3: Editor reads hints and concludes
    bb.claim_intent("i_hypothesis", "agent-d").unwrap();

    let state = bb.read_state();
    assert_eq!(state.hints.len(), 2, "2 review hints submitted");
    assert!(
        state.hints.iter().any(|h| h.creator == "agent-b"),
        "hint from agent-b"
    );

    // Editor concludes incorporating feedback
    let result = bb.conclude_intent(
        "i_hypothesis",
        "Hypothesis accepted with revisions: surface code QEC at distance-3 achieves 0.1% threshold (confirmed). Distance-5 requires 0.05%.",
    ).unwrap();
    assert!(
        result
            .content
            .as_str()
            .unwrap_or("")
            .contains("accepted with revisions")
    );

    // Verify via Cypher
    let hint_count = bb.with_graph(|g| {
        let p = cypher::Plan::from_internal("MATCH (h:Hint) RETURN h").unwrap();
        cypher::execute(g, &p).unwrap().len()
    });
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
    let bb = HybridBlackboard::new();

    // Three agents each submit partial observations
    let pieces = [
        (
            "f_piece_1",
            "agent-alpha",
            "Battery cell temperature rises 15°C under 2C discharge rate",
        ),
        (
            "f_piece_2",
            "agent-beta",
            "Electrolyte viscosity doubles below -10°C ambient temperature",
        ),
        (
            "f_piece_3",
            "agent-gamma",
            "Anode lithium plating occurs at charge rates above 1C below 0°C",
        ),
    ];
    for (id, creator, content) in &pieces {
        bb.submit_fact(&Fact {
            id: FihHash::from_hex(id),
            origin: "experiment".into(),
            content: (*content).into(),
            creator: creator.to_string(),
        })
        .unwrap();
    }

    // Agent-D reads all and synthesizes
    let state = bb.read_state();
    assert_eq!(state.facts.len(), 3);

    // Agent-D recognizes the pattern: cold + high rate = lithium plating
    bb.submit_fact(&Fact {
        id: FihHash::from_hex("f_synthesis"),
        origin: "synthesis".into(),
        content: "SYNTHESIS: Low temperature (-10°C) increases electrolyte viscosity, reducing ion mobility. High discharge rate (2C) generates heat (15°C rise). Combined high charge rate (>1C) below 0°C causes anode lithium plating. Solution: preheat battery to 10°C before fast charging in cold environments.".into(),
        creator: "agent-delta".into(),
    }).unwrap();

    // Verify synthesis
    let state = bb.read_state();
    assert_eq!(state.facts.len(), 4, "3 pieces + 1 synthesis");
    let synthesis = &state.facts[3];
    assert!(
        synthesis
            .content
            .as_str()
            .unwrap_or("")
            .contains("SYNTHESIS"),
        "synthesis marker present"
    );
    assert!(
        synthesis.content.as_str().unwrap_or("").contains("preheat"),
        "actionable solution proposed"
    );

    // Agent-D's synthesis should be propositional — others can build on it
    bb.submit_intent(&Intent {
        id: FihHash::from_hex("i_validate_synthesis"),
        from_facts: vec![FihHash::from_hex("f_synthesis")],
        description:
            "Experimental validation: test preheat to 10°C before 2C charging at -10°C ambient"
                .into(),
        creator: "agent-epsilon".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    })
    .unwrap();

    let state = bb.read_state();
    assert_eq!(state.intents.len(), 1, "validation intent submitted");

    // Cypher: all pieces accessible
    let count = bb.with_graph(|g| {
        let p = cypher::Plan::from_internal("MATCH (f:Fact) RETURN f").unwrap();
        cypher::execute(g, &p).unwrap().len()
    });
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
    let bb = HybridBlackboard::new();

    // Sensor agents detect anomalies
    let alerts = [
        (
            "f_alarm_smoke",
            "sensor-alpha",
            "Smoke detected in sector 7, visibility 2m",
        ),
        (
            "f_alarm_temp",
            "sensor-beta",
            "Temperature spike 85°C in sector 7, rising 2°/min",
        ),
        (
            "f_alarm_power",
            "sensor-gamma",
            "Power line to sector 7 shows 40% voltage drop",
        ),
    ];
    for (id, creator, content) in &alerts {
        bb.submit_fact(&Fact {
            id: FihHash::from_hex(id),
            origin: "sensor".into(),
            content: (*content).into(),
            creator: creator.to_string(),
        })
        .unwrap();
    }

    // Coordinator submits prioritized response plan as Intent
    bb.submit_intent(&Intent {
        id: FihHash::from_hex("i_respond_fire"),
        from_facts: vec![
            FihHash::from_hex("f_alarm_smoke"),
            FihHash::from_hex("f_alarm_temp"),
            FihHash::from_hex("f_alarm_power"),
        ],
        description: "FIRE_RESPONSE: Evacuate sector 7, activate fire suppression, isolate power"
            .into(),
        creator: "coordinator".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    })
    .unwrap();

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
    let outcome = bb
        .conclude_intent(
            "i_respond_fire",
            "Sector 7 evacuated, fire suppressed in 3min, power isolated. No casualties.",
        )
        .unwrap();
    assert!(outcome.content.as_str().unwrap_or("").contains("evacuated"));

    // Final state
    let state = bb.read_state();
    assert_eq!(state.facts.len(), 4, "3 alerts + 1 outcome");
    assert_eq!(state.intents.len(), 1, "response intent");

    println!("  ✓ Emergency Response: 3 sensors + coordinator + 3 responders = 7 agents");
    println!("  ✓ Competitive claim: only one responder wins, others rejected");
    println!("  ✓ Full lifecycle: alert → plan → claim → heartbeat → conclude");
}

// ── Scenario 5: Bug Triage & Fix Pipeline (실무 산업) ────────────────────
//
// Reporter files a critical bug → Triager classifies → Developer claims and fixes
// → Reviewer validates → Fix concluded and deployed.
// 5 agents, 3 intents (triage → fix → review), full lifecycle.

#[test]
fn scenario_bug_fix_pipeline() {
    let bb = HybridBlackboard::new();

    // Reporter submits the bug as a Fact
    bb.submit_fact(&Fact {
        id: FihHash::from_hex("f_bug_1337"),
        origin: "production".into(),
        content: "CRITICAL: Payment API returns 500 for amounts > $10,000 since deploy v2.3.1 at 2026-06-15T14:32Z. Affects 12% of enterprise transactions.".into(),
        creator: "reporter".into(),
    }).unwrap();

    // Triager reads the bug, adds metadata as Hint, files a triage Intent
    bb.submit_hint(&Hint {
        id: FihHash::from_hex("h_severity"),
        content: "severity=P0, component=payment-api, regression=true".into(),
        creator: "triager".into(),
    })
    .unwrap();
    bb.submit_intent(&Intent {
        id: FihHash::from_hex("i_triage"),
        from_facts: vec![FihHash::from_hex("f_bug_1337")],
        description: "TRIAGE: Payment API overflow on large amounts — check decimal handling"
            .into(),
        creator: "triager".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    })
    .unwrap();

    // Developer claims triage, concludes with analysis
    bb.claim_intent("i_triage", "dev-alice").unwrap();
    let analysis = bb.conclude_intent("i_triage",
        "Root cause: amount field uses uint32 (max $42,949.67). Amounts > $10K approach limit with tax/shipping. Fix: migrate to uint64. Estimated effort: 2h."
    ).unwrap();
    assert!(analysis.content.as_str().unwrap_or("").contains("uint32"));

    // Developer submits a fix Intent
    bb.submit_intent(&Intent {
        id: FihHash::from_hex("i_fix_1337"),
        from_facts: vec![FihHash::from_hex("f_bug_1337"), analysis.id],
        description: "FIX: Change payment amount from uint32 to uint64 in api/src/payment.rs"
            .into(),
        creator: "dev-alice".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    })
    .unwrap();

    // Developer claims and implements the fix
    bb.claim_intent("i_fix_1337", "dev-alice").unwrap();
    bb.conclude_intent("i_fix_1337",
        "Fix deployed: uint32→uint64 migration complete. Tested with $99,999.99 transaction (PASS). PR #2137 merged."
    ).unwrap();

    // Reviewer submits a review Intent to validate
    bb.submit_intent(&Intent {
        id: FihHash::from_hex("i_review_1337"),
        from_facts: vec![FihHash::from_hex("f_bug_1337")],
        description: "REVIEW: Verify fix covers edge cases — negative amounts, fractional cents, max uint64 boundary".into(),
        creator: "reviewer-bob".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    }).unwrap();
    bb.claim_intent("i_review_1337", "reviewer-bob").unwrap();
    let verdict = bb.conclude_intent("i_review_1337",
        "REVIEW PASSED: edge cases handled. uint64 max ($184M) sufficient. Negative inputs rejected by schema validation."
    ).unwrap();
    assert!(verdict.content.as_str().unwrap_or("").contains("PASSED"));

    let state = bb.read_state();
    assert_eq!(
        state.facts.len(),
        4,
        "1 bug report + 3 conclusion facts = 4"
    );
    assert_eq!(state.hints.len(), 1, "severity hint from triager");
    assert!(
        state
            .intents
            .iter()
            .any(|i| i.id == FihHash::from_hex("i_fix_1337")),
        "fix intent present"
    );

    println!("  ✓ Bug Pipeline: reporter→triager→dev→reviewer = 4 agents, 7 FIH ops");
    println!("  ✓ P0 bug: submitted → triaged → fixed → reviewed → passed");
    println!("  ✓ Hints carry severity metadata across agent boundaries");
}

// ── Scenario 6: CI/CD Failure Investigation (실무 DevOps) ────────────────
//
// A build breaks. 3 specialists each investigate a different dimension:
//   - Agent-A: compile errors
//   - Agent-B: test failures
//   - Agent-C: dependency versions
// They collaborate to find root cause, then D proposes a fix.

#[test]
fn scenario_ci_failure_investigation() {
    let bb = HybridBlackboard::new();

    // CI system reports build failure
    bb.submit_fact(&Fact {
        id: FihHash::from_hex("f_build_404"),
        origin: "ci".into(),
        content: "BUILD FAILED: main branch, commit a1b2c3d, pipeline #8421. 23 test failures, 5 compile errors, 3 dependency warnings.".into(),
        creator: "ci-bot".into(),
    }).unwrap();

    // Agent-A investigates compile errors
    bb.submit_fact(&Fact {
        id: FihHash::from_hex("f_compile"),
        origin: "investigation".into(),
        content: "Compile errors: all 5 are in protocol/buffer.rs — 'PacketHeader' struct size mismatch after adding new field. Missing #[repr(C)] attribute.".into(),
        creator: "agent-a".into(),
    }).unwrap();

    // Agent-B investigates test failures (independently, same time)
    bb.submit_fact(&Fact {
        id: FihHash::from_hex("f_tests"),
        origin: "investigation".into(),
        content: "Test failures: 23/23 are serialization round-trip tests. All fail with 'buffer size mismatch'. Consistent with struct layout change.".into(),
        creator: "agent-b".into(),
    }).unwrap();

    // Agent-C checks dependencies
    bb.submit_fact(&Fact {
        id: FihHash::from_hex("f_deps"),
        origin: "investigation".into(),
        content: "Dependencies: proto-rs v2.4.0 released yesterday — includes automated PacketHeader generator that changed alignment. No API change, but layout differs.".into(),
        creator: "agent-c".into(),
    }).unwrap();

    // Agent-D reads ALL investigations, identifies root cause
    let state = bb.read_state();
    assert_eq!(state.facts.len(), 4, "build report + 3 investigations");

    // Agent-D triangulates: compile error (repr(C)) + test failure (size) + dep update (alignment)
    // = root cause: proto-rs v2.4.0 changed struct alignment
    bb.submit_intent(&Intent {
        id: FihHash::from_hex("i_root_cause"),
        from_facts: vec![FihHash::from_hex("f_compile"), FihHash::from_hex("f_tests"), FihHash::from_hex("f_deps")],
        description: "ROOT CAUSE: proto-rs v2.4.0 automated PacketHeader generator produces different alignment than manual #[repr(C)] struct. 3 independent signals converge.".into(),
        creator: "agent-d".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    }).unwrap();
    bb.claim_intent("i_root_cause", "agent-d").unwrap();
    let diagnosis = bb.conclude_intent("i_root_cause",
        "Root cause confirmed: proto-rs v2.4.0 alignment change. Fix: pin proto-rs to v2.3.9 in Cargo.toml, or add #[repr(C)] to generated code. Pinning is 5min fix."
    ).unwrap();
    assert!(
        diagnosis
            .content
            .as_str()
            .unwrap_or("")
            .contains("proto-rs v2.4.0")
    );

    let state = bb.read_state();
    assert_eq!(state.facts.len(), 5, "4 reports + 1 diagnosis");
    assert!(state.intents.len() >= 1, "diagnosis intent");

    println!("  ✓ CI Failure: 4 agents investigate independently, converge on root cause");
    println!("  ✓ Each agent saw different symptoms → same root cause via Blackboard");
    println!("  ✓ Evidence triangulation without direct agent communication");
}

// ── Scenario 7: Supply Chain Incident Response (실무 운영) ───────────────
//
// A security vulnerability is disclosed in a critical dependency.
// Multiple teams coordinate response: security assessment, mitigation,
// communication, and post-mortem.

#[test]
fn scenario_supply_chain_incident() {
    let bb = HybridBlackboard::new();

    // Security advisory published (external trigger)
    bb.submit_fact(&Fact {
        id: FihHash::from_hex("f_advisory_GHSA"),
        origin: "github-advisory".into(),
        content: "CRITICAL: CVE-2026-4413 in openssl-sys v0.9.100 — remote buffer overflow in TLS handshake. CVSS 9.8. Affects all services using TLS.".into(),
        creator: "security-bot".into(),
    }).unwrap();

    // Security team assesses blast radius
    bb.submit_intent(&Intent {
        id: FihHash::from_hex("i_assess"),
        from_facts: vec![FihHash::from_hex("f_advisory_GHSA")],
        description: "ASSESS: Inventory all services using openssl-sys < 0.9.101".into(),
        creator: "sec-lead".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    })
    .unwrap();
    bb.claim_intent("i_assess", "sec-lead").unwrap();
    let impact = bb.conclude_intent("i_assess",
        "Impact: 12 microservices use openssl-sys. 8 are edge-facing (critical). 4 are internal (medium). All need patching."
    ).unwrap();
    assert!(
        impact
            .content
            .as_str()
            .unwrap_or("")
            .contains("12 microservices")
    );

    // SRE team plans mitigation (parallel track, reads sec-lead's conclusion)
    bb.submit_intent(&Intent {
        id: FihHash::from_hex("i_mitigate"),
        from_facts: vec![FihHash::from_hex("f_advisory_GHSA"), impact.id.clone()],
        description: "MITIGATE: Update openssl-sys to 0.9.101 across all services".into(),
        creator: "sre-lead".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    })
    .unwrap();
    bb.claim_intent("i_mitigate", "sre-lead").unwrap();
    let patch = bb.conclude_intent("i_mitigate",
        "Patch: openssl-sys bumped to 0.9.101 in all 12 services. 8 edge services patched via rolling update (zero downtime). 4 internal services updated. No regressions."
    ).unwrap();
    assert!(
        patch
            .content
            .as_str()
            .unwrap_or("")
            .contains("rolling update")
    );

    // Communications team drafts announcement
    bb.submit_intent(&Intent {
        id: FihHash::from_hex("i_comms"),
        from_facts: vec![
            FihHash::from_hex("f_advisory_GHSA"),
            impact.id.clone(),
            patch.id,
        ],
        description: "COMMS: Draft security advisory for customers".into(),
        creator: "comms-lead".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    })
    .unwrap();
    bb.claim_intent("i_comms", "comms-lead").unwrap();
    let advisory = bb.conclude_intent("i_comms",
        "Advisory published: CVE-2026-4413 patched within 4h of disclosure. No customer impact. Post-mortem scheduled."
    ).unwrap();
    assert!(advisory.content.as_str().unwrap_or("").contains("4h"));

    // Post-mortem lead submits findings
    bb.submit_hint(&Hint {
        id: FihHash::from_hex("h_postmortem"),
        content: "Post-mortem action: add openssl-sys to automated dependency vulnerability scanner. ETA: 1 week.".into(),
        creator: "pm-lead".into(),
    }).unwrap();

    let state = bb.read_state();
    assert_eq!(state.facts.len(), 4, "1 advisory + 3 conclusion facts = 4");
    assert_eq!(state.hints.len(), 1, "post-mortem action item");
    assert_eq!(
        state.intents.len(),
        3,
        "assess + mitigate + comms = 3 intents"
    );

    println!("  ✓ Supply Chain Incident: security→SRE→comms→post-mortem = 4 tracks, parallel");
    println!("  ✓ CVE-2026-4413: disclosed → assessed → patched → communicated in 4h");
    println!("  ✓ Each track reads previous conclusions via read_state() — no direct calls");
}

// ── Scenario 8: SSCCS Computational Primitive Discovery (연구 실무) ──────
//
// Multiple research agents analyze the llvm-project to discover and formalize
// the "Segment" computational primitive — a self-contained unit of computation
// with typed boundaries. Each agent examines a different dimension:
//   - Agent-A: memory access patterns (spatial locality)
//   - Agent-B: data flow graphs (temporal locality)
//   - Agent-C: control flow structure (boundary identification)
//   - Agent-D: formalizes the discovered primitive into Segment/Scheme/Field
//
// This mirrors the actual SSCCS research methodology: structural observation
// across multiple compiler IRs to extract universal computation primitives.

#[test]
fn scenario_ssccs_primitive_discovery() {
    let bb = HybridBlackboard::new();

    // ── Phase 1: Agents observe different IRs ─────────────────────────

    // Agent-A analyzes memory access patterns in LLVM IR
    bb.submit_fact(&Fact {
        id: FihHash::from_hex("f_memory_pattern"),
        origin: "llvm-ir".into(),
        content: "OBSERVATION: 73% of loads/stores in hot loops access consecutive addresses (stride=1). 18% show strided access (stride=4/8). 9% are gather/scatter. Spatial locality is the dominant pattern, not random access.".into(),
        creator: "agent-a".into(),
    }).unwrap();

    // Agent-B analyzes data flow graphs in MLIR
    bb.submit_fact(&Fact {
        id: FihHash::from_hex("f_dataflow_pattern"),
        origin: "mlir".into(),
        content: "OBSERVATION: Data flow subgraphs show strong temporal locality — 89% of SSA values are used within 12 instructions of definition. Def-use chains form natural clusters: compute kernels, memory fences, control boundaries.".into(),
        creator: "agent-b".into(),
    }).unwrap();

    // Agent-C analyzes control flow structure (CFG)
    bb.submit_fact(&Fact {
        id: FihHash::from_hex("f_cfg_pattern"),
        origin: "cfg-analysis".into(),
        content: "OBSERVATION: CFG natural loops have clear entry/exit points. 94% of basic blocks belong to exactly one loop nest. Loop boundaries are stable across optimization passes — they are structural invariants of the computation, not artifacts.".into(),
        creator: "agent-c".into(),
    }).unwrap();

    let state = bb.read_state();
    assert_eq!(state.facts.len(), 3, "3 observations from different IRs");
    println!("  Phase 1: 3 agents observed distinct structural patterns across LLVM/MLIR/CFG");

    // ── Phase 2: Agents cross-reference and detect convergence ────────

    // Agent-A notes: memory stride patterns match loop boundaries from Agent-C
    bb.submit_hint(&Hint {
        id: FihHash::from_hex("h_convergence_1"),
        content: "CROSS-REF: Memory stride=1 patterns align with innermost loops (Agent-C's observation). The memory access unit IS the loop body — a natural computational boundary.".into(),
        creator: "agent-a".into(),
    }).unwrap();

    // Agent-B notes: SSA def-use clusters match memory regions from Agent-A
    bb.submit_hint(&Hint {
        id: FihHash::from_hex("h_convergence_2"),
        content: "CROSS-REF: SSA value clusters (12-instruction window) correspond to memory stride-1 regions (Agent-A). The data flow cluster and memory access region share the same boundary — this is NOT coincidence.".into(),
        creator: "agent-b".into(),
    }).unwrap();

    // Agent-C notes: all three dimensions converge on the same structural unit
    bb.submit_fact(&Fact {
        id: FihHash::from_hex("f_convergence"),
        origin: "cross-reference".into(),
        content: "CONVERGENCE: Memory (spatial), data flow (temporal), and control flow (structural) all identify the same atomic unit: a self-contained loop nest with bounded memory access and localized def-use chains. This unit is universal across the three IRs.".into(),
        creator: "agent-c".into(),
    }).unwrap();

    println!("  Phase 2: 3 cross-references confirm convergence — a universal atomic unit");

    // ── Phase 3: Agent-D formalizes the discovered primitive ──────────

    // Agent-D reads all observations and convergence hints, then submits a formalization Intent
    bb.submit_intent(&Intent {
        id: FihHash::from_hex("i_formalize_segment"),
        from_facts: vec![
            FihHash::from_hex("f_memory_pattern"),
            FihHash::from_hex("f_dataflow_pattern"),
            FihHash::from_hex("f_cfg_pattern"),
            FihHash::from_hex("f_convergence"),
        ],
        description: "FORMALIZE: Define 'Segment' as the universal atomic computation unit with typed boundaries (spatial: memory stride, temporal: def-use window, structural: loop entry/exit).".into(),
        creator: "agent-d".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    }).unwrap();

    // Agent-D claims and concludes with the formal definition
    bb.claim_intent("i_formalize_segment", "agent-d").unwrap();
    let segment_def = bb
        .conclude_intent(
            "i_formalize_segment",
            "SEGMENT FORMALIZED: A Segment is a triple (M, D, B) where:
  M: memory access region (contiguous address range, stride pattern)
  D: def-use chain cluster (SSA value window, data flow subgraph)
  B: structural boundary (loop entry/exit, CFG natural loop)

Properties:
  - Self-contained: all inputs enter at B_entry, all outputs exit at B_exit
  - Compositional: Segments nest hierarchically (loop nest → function → module)
  - Observable: M, D, B are independently measurable across any IR
  - Universal: present in LLVM IR, MLIR, CFG, and downstream to machine code

Implication: The Segment is a computational primitive — an atom of computation
that the von Neumann architecture can be redesigned around.",
        )
        .unwrap();
    assert!(
        segment_def
            .content
            .as_str()
            .unwrap_or("")
            .contains("SEGMENT FORMALIZED")
    );
    assert!(
        segment_def
            .content
            .as_str()
            .unwrap_or("")
            .contains("Universal")
    );

    println!(
        "  Phase 3: Agent-D formalized the 'Segment' primitive from 4 converging observations"
    );

    // ── Phase 4: Peer validation ──────────────────────────────────────

    // Agent-A validates the Segment definition against known memory patterns
    bb.submit_intent(&Intent {
        id: FihHash::from_hex("i_validate_segment"),
        from_facts: vec![FihHash::from_hex("f_memory_pattern"), segment_def.id.clone()],
        description: "VALIDATE: Does the Segment definition predict the 73/18/9 memory access distribution? If M is a contiguous stride-1 region, it should also explain strided and gather/scatter cases.".into(),
        creator: "agent-a".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    }).unwrap();
    bb.claim_intent("i_validate_segment", "agent-a").unwrap();
    let validation = bb.conclude_intent("i_validate_segment",
        "VALIDATION PASSED: Segment model predicts stride-1 (73% = innermost loops), stride-4/8 (18% = struct field access across Segment boundaries), gather/scatter (9% = cross-Segment irregular access). The 73/18/9 distribution is a natural consequence of hierarchical Segment composition."
    ).unwrap();
    assert!(validation.content.as_str().unwrap_or("").contains("PASSED"));

    // Agent-E (new observer) reads the full thread and proposes a Scheme
    bb.submit_intent(&Intent {
        id: FihHash::from_hex("i_scheme_definition"),
        from_facts: vec![segment_def.id],
        description: "SCHEME: Define 'Scheme' as a typed transformation between Segments. Scheme(S1, S2, T) where T is the transformation type (map, reduce, shuffle, broadcast). This enables algebraic reasoning about Segment compositions.".into(),
        creator: "agent-e".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    }).unwrap();
    bb.claim_intent("i_scheme_definition", "agent-e").unwrap();
    let scheme_def = bb
        .conclude_intent(
            "i_scheme_definition",
            "SCHEME FORMALIZED: Scheme(S₁, S₂, T) where:
  - S₁, S₂ are Segments
  - T ∈ {map, reduce, shuffle, broadcast, fuse, split}
  - A Scheme preserves the Segment properties (self-contained, compositional)

This completes the first two layers of the SSCCS ontology: Segment + Scheme.",
        )
        .unwrap();
    assert!(
        scheme_def
            .content
            .as_str()
            .unwrap_or("")
            .contains("SCHEME FORMALIZED")
    );

    let state = bb.read_state();
    // 3 observations + 1 convergence + 1 formalization + 1 validation + 1 scheme = 7
    assert_eq!(
        state.facts.len(),
        7,
        "3 observations + convergence + formalization + validation + scheme"
    );
    assert_eq!(state.hints.len(), 2, "2 cross-reference hints");
    assert_eq!(
        state.intents.len(),
        3,
        "formalize + validate + scheme = 3 intents"
    );

    println!();
    println!("  ✓ SSCCS Primitive Discovery: 5 agents, 2 FIH layers (Segment + Scheme)");
    println!("  ✓ 3 independent IR observations → convergence → formalization → validation");
    println!("  ✓ Full SSCCS ontology derivation through collaborative FIH inference");
    println!("  ✓ Key insight: memory access + data flow + control flow = the same universal unit");
}
