// Nexus Real-World Scenario Tests
// ================================
// Simulates how Nexus is consumed in actual SSCCS research workflows.
// Uses claims from real documents at docs.ssccs.org.
//
// These scenarios validate that the detection layer works on realistic,
// cross-domain document collections — not just synthetic test data.
//
// FIH Flow demonstrated in each scenario:
//   1. submit_fact(document_claim)     ← knowledge ingestion
//   2. detector.orient(state)          ← automated pattern observation
//   3. detector → Fact(gap|contradiction|state_change)  ← immutable record
//   4. agent reads detector Facts      ← stigmergy: agents perceive traces
//   5. agent → submit_intent()         ← agent proposes action
//   6. agent → claim → conclude        ← FIH lifecycle completion
//   7. new synthesis Fact created      ← knowledge grows
//
// Scenarios:
//   1. Cross-domain discovery (manifesto + spatz + eurollvm)
//      — philosophy meets hardware meets compiler infrastructure
//   2. Peer review challenge (rust_zerocost challenges manifesto purity)
//      — practical implementation questions ontological claims
//   3. Incremental knowledge growth (many iterations, emergent patterns)
//      — detectors produce facts → agents create intents → new facts → repeat
//   4. Multi-agent collaboration (agents read detector facts, create intents)
//      — 3 specialized agents, no direct communication, stigmergy only
//   5. Document revision (v1 → detector facts → v2 arrives → state change)
//      — knowledge evolution tracked through detector observations

use nex::create_blackboard;
use nex::process::scheduler::Scheduler;
use nex::process::tasks::contradiction_detector::ContradictionDetector;
use nex::process::tasks::gap_detector::GapDetector;
use nex::process::tasks::new_document_analyzer::NewDocumentAnalyzer;
use nex::process::tasks::state_change_detector::StateChangeDetector;
use nexus_model::{
    Blackboard, BoardState, Content, EvictCapable, Fact, FactCapable, FihHash, Intent,
    IntentCapable, StorageRead,
};
use nexus_storage_composite::HybridBlackboard;
use nexus_storage_petgraph::{Snapshottable, StorageSnapshot};

// ── Helpers ────────────────────────────────────────────────────────────

fn claim(id: &str, origin: &str, claim_text: &str, topic: &str, position: &str) -> Fact {
    Fact {
        id: FihHash::from_hex(id),
        origin: origin.to_string(),
        content: Content {
            mime_type: "application/json".into(),
            data: serde_json::to_string(
                &serde_json::json!({ "claim": claim_text, "topic": topic, "position": position }),
            )
            .unwrap_or_default()
            .into_bytes(),
        },
        creator: "ingester".into(),
    }
}

fn count_by_creator(state: &BoardState, creator: &str) -> usize {
    state.facts.iter().filter(|f| f.creator == creator).count()
}

fn content_val_of(f: &Fact) -> serde_json::Value {
    serde_json::from_str(f.content.as_str().unwrap_or("")).unwrap_or(serde_json::Value::Null)
}

fn count_by_type(state: &BoardState, fact_type: &str) -> usize {
    state
        .facts
        .iter()
        .filter(|f| content_val_of(f).get("type").and_then(|v| v.as_str()) == Some(fact_type))
        .count()
}

struct TickResult {
    #[allow(dead_code)]
    facts_submitted: usize,
    state: BoardState,
}

fn do_tick(sched: &mut Scheduler<impl Blackboard + EvictCapable>) -> TickResult {
    let facts_submitted = sched.tick().expect("tick");
    let state = StorageRead::read_state(&sched.bb);
    TickResult {
        facts_submitted,
        state,
    }
}

// ═════════════════════════════════════════════════════════════════════════
//  Scenario 1: Cross-Domain Discovery
//  manifesto (philosophy) + spatz (hardware) + eurollvm (compiler)
//  Detects gaps between abstract claims and concrete implementations
// ═════════════════════════════════════════════════════════════════════════

fn seed_cross_domain(bb: &mut impl Blackboard) -> Vec<String> {
    let facts = [
        // manifesto.llms.md — philosophical claims
        claim(
            "p01",
            "manifesto.llms.md",
            "Computation is the collapse of structured potential",
            "computation-ontology",
            "collapse-based",
        ),
        claim(
            "p02",
            "manifesto.llms.md",
            "Data remains stationary — zero movement of input data",
            "data-movement",
            "zero-movement",
        ),
        claim(
            "p03",
            "manifesto.llms.md",
            "Parallelism emerges from structural independence",
            "parallelism",
            "structural-emergent",
        ),
        claim(
            "p04",
            "manifesto.llms.md",
            "Performance is not the first objective; structural fidelity is",
            "design-priority",
            "fidelity-first",
        ),
        // spatz_insight.llms.md — hardware constraints
        claim(
            "s01",
            "spatz_insight.llms.md",
            "Data movement is transformed, not eliminated — it becomes structured dataflow",
            "data-movement",
            "structured-dataflow",
        ),
        claim(
            "s02",
            "spatz_insight.llms.md",
            "Performance is bandwidth-limited, not compute-limited",
            "performance-model",
            "bandwidth-limited",
        ),
        claim(
            "s03",
            "spatz_insight.llms.md",
            "Optimal register capacity Z_opt ≈ (C_F · β)^2; excess wastes energy",
            "state-size",
            "bounded-optimal",
        ),
        // eurollvm26_insight.llms.md — compiler infrastructure
        claim(
            "e01",
            "eurollvm26_insight.llms.md",
            "Canonicalization has no termination guarantee — canonicalize does not canonicalize",
            "compiler-correctness",
            "no-guarantee",
        ),
        claim(
            "e02",
            "eurollvm26_insight.llms.md",
            "MLIR Transform Dialect separates payload IR from transformation IR",
            "compiler-architecture",
            "separation-of-concerns",
        ),
        claim(
            "e03",
            "eurollvm26_insight.llms.md",
            "SSCCS should adopt Melior for Rust-MLIR integration",
            "compiler-implementation",
            "rust-mlir-hybrid",
        ),
        claim(
            "e04",
            "eurollvm26_insight.llms.md",
            "Static analysis can verify structural properties of Schemes",
            "compiler-correctness",
            "static-verification",
        ),
    ];
    let ids: Vec<String> = facts.iter().map(|f| f.id.to_string()).collect();
    for f in &facts {
        bb.submit_fact(f).unwrap();
    }
    ids
}

#[test]
fn scenario_cross_domain_discovery() {
    let mut bb = create_blackboard();
    let baseline = seed_cross_domain(&mut bb);

    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));
    sched.register(Box::new(ContradictionDetector::new()));
    sched.register(Box::new(NewDocumentAnalyzer::with_baseline(baseline)));

    // Tick 1: gap + contradiction detection on cross-domain corpus
    let r1 = do_tick(&mut sched);

    // Gap facts: cross-origin gaps between philosophy/hardware/compiler domains
    let gap_facts = count_by_creator(&r1.state, "gap-detector");
    assert!(gap_facts > 0, "Cross-domain gaps detected: {}", gap_facts);

    // Contradiction: "data-movement" topic — zero-movement vs structured-dataflow
    let contradictions = count_by_type(&r1.state, "contradiction");
    assert!(
        contradictions > 0,
        "Contradictions found: {}",
        contradictions
    );

    // Verify the data-movement contradiction specifically
    let has_data_movement_contradiction = r1
        .state
        .facts
        .iter()
        .filter(|f| f.creator == "contradiction-detector")
        .any(|f| content_val_of(f).get("topic").and_then(|v| v.as_str()) == Some("data-movement"));
    assert!(
        has_data_movement_contradiction,
        "data-movement contradiction: zero-movement (manifesto) vs structured-dataflow (spatz)"
    );

    // Agent reads contradiction fact, creates intent to resolve
    let contradiction_facts: Vec<_> = r1
        .state
        .facts
        .iter()
        .filter(|f| f.creator == "contradiction-detector")
        .collect();
    for cf in &contradiction_facts {
        let intent = Intent {
            id: FihHash::new(&[&cf.id.to_string(), "resolve"], "intent"),
            from_facts: vec![cf.id.clone()],
            description: format!(
                "Resolve: {}",
                content_val_of(cf)
                    .get("topic")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
            ),
            creator: "research-agent".into(),
            worker: None,
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            is_concluded: false,
            concluded_at: None,
        };
        let iid = sched.bb.submit_intent(&intent).expect("submit");
        sched
            .bb
            .claim_intent(&iid.to_string(), "research-agent")
            .expect("claim");
        sched.bb.conclude_intent(&iid.to_string(), &serde_json::to_string(&serde_json::json!({
            "resolution": "Data movement is zero for input Segments; projection results move as structured dataflow bounded by Spatz balance condition"
        })).unwrap()).expect("conclude");
    }

    let state = StorageRead::read_state(&sched.bb);
    // Original + detector facts + conclusions
    assert!(
        state.facts.len() > 11,
        "Knowledge grew: {} facts",
        state.facts.len()
    );
    // Intents were concluded (may still be present if not evicted)
    assert!(
        state.intents.len() >= contradiction_facts.len(),
        "Intents created: {}",
        state.intents.len()
    );
}

// ═════════════════════════════════════════════════════════════════════════
//  Scenario 2: Peer Review Challenge
//  rust_zerocost document challenges SSCCS manifesto purity claims
// ═════════════════════════════════════════════════════════════════════════

fn seed_manifesto_base(bb: &mut impl Blackboard) -> Vec<String> {
    let facts = [
        claim(
            "m01",
            "manifesto.llms.md",
            "Computation is the collapse of structured potential",
            "computation-ontology",
            "collapse-based",
        ),
        claim(
            "m02",
            "manifesto.llms.md",
            "Segments are immutable, stateless coordinate points",
            "segment-property",
            "immutable-coordinate",
        ),
        claim(
            "m03",
            "manifesto.llms.md",
            "There is no instruction stream — only structure and its collapse",
            "execution-model",
            "collapse-based",
        ),
        claim(
            "m04",
            "manifesto.llms.md",
            "Performance is not the first objective; structural fidelity is",
            "design-priority",
            "fidelity-first",
        ),
    ];
    let ids: Vec<String> = facts.iter().map(|f| f.id.to_string()).collect();
    for f in &facts {
        bb.submit_fact(f).unwrap();
    }
    ids
}

fn seed_rust_challenge(bb: &mut impl Blackboard) {
    let facts = [
        // rust_zerocost document — practical implementation challenges
        claim(
            "r01",
            "rust_zerocost.llms.md",
            "Rust's zero-cost abstractions allow complex logical structures without runtime overhead",
            "computation-ontology",
            "practical-abstraction",
        ),
        claim(
            "r02",
            "rust_zerocost.llms.md",
            "Rust ownership and immutability align perfectly with SSCCS segment immutability",
            "segment-property",
            "immutable-coordinate",
        ),
        claim(
            "r03",
            "rust_zerocost.llms.md",
            "Even with Rust, data must travel from L3 cache to registers — physical constraints remain",
            "execution-model",
            "hardware-bound",
        ),
        claim(
            "r04",
            "rust_zerocost.llms.md",
            "Rust already outperforms traditional languages on existing hardware",
            "design-priority",
            "practical-performance",
        ),
        claim(
            "r05",
            "rust_zerocost.llms.md",
            "The same .ss file can compile to native now, map to PIM/OCP later",
            "future-proofing",
            "dual-path",
        ),
    ];
    for f in &facts {
        bb.submit_fact(f).unwrap();
    }
}

#[test]
fn scenario_peer_review_challenge() {
    let mut bb = create_blackboard();
    let baseline = seed_manifesto_base(&mut bb);

    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(ContradictionDetector::new()));
    sched.register(Box::new(GapDetector::new()));
    sched.register(Box::new(NewDocumentAnalyzer::with_baseline(baseline)));

    // Phase 1: Analyze manifesto base — should find gaps but no contradictions yet
    let _r1 = do_tick(&mut sched);

    // Phase 2: Rust challenge document arrives
    seed_rust_challenge(&mut sched.bb);
    let r2 = do_tick(&mut sched);

    // NDA should find +factors (supports) and -factors (challenges)
    let nda_facts: Vec<_> = r2
        .state
        .facts
        .iter()
        .filter(|f| f.creator == "new-document-analyzer")
        .collect();
    let factor_of = |f: &&Fact| -> Option<String> {
        content_val_of(f)
            .get("factor")?
            .as_str()
            .map(|s| s.to_string())
    };
    let supports = nda_facts
        .iter()
        .filter(|f| factor_of(f).as_deref() == Some("+factor"))
        .count();
    let challenges = nda_facts
        .iter()
        .filter(|f| factor_of(f).as_deref() == Some("-factor"))
        .count();
    let gaps = nda_facts
        .iter()
        .filter(|f| factor_of(f).as_deref() == Some("gap"))
        .count();

    assert!(
        supports > 0,
        "+factors found (rust aligns with manifesto): {}",
        supports
    );
    assert!(
        challenges > 0,
        "-factors found (rust challenges manifesto): {}",
        challenges
    );
    assert!(
        gaps > 0,
        "New topics discovered (future-proofing, dual-path): {}",
        gaps
    );

    // Contradiction: "computation-ontology" — collapse-based vs practical-abstraction
    let contradictions = count_by_type(&r2.state, "contradiction");
    assert!(
        contradictions > 0,
        "Peer review contradictions: {}",
        contradictions
    );

    // Agent: synthesize peer review conclusion
    let contradiction_fact = r2.state.facts.iter().find(|f| {
        f.creator == "contradiction-detector"
            && content_val_of(f).get("topic").and_then(|v| v.as_str())
                == Some("computation-ontology")
    });
    if let Some(cf) = contradiction_fact {
        let intent = Intent {
            id: FihHash::new(&[&cf.id.to_string(), "peer-review"], "intent"),
            from_facts: vec![cf.id.clone()],
            description: "Peer review: resolve computation-ontology contradiction".into(),
            creator: "reviewer".into(),
            worker: None,
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            is_concluded: false,
            concluded_at: None,
        };
        let iid = sched.bb.submit_intent(&intent).expect("submit");
        sched.bb.claim_intent(&iid.to_string(), "reviewer").expect("claim");
        sched.bb.conclude_intent(&iid.to_string(), &serde_json::to_string(&serde_json::json!({
            "verdict": "Rust is the practical stepping stone; SSCCS manifesto describes the destination. Both are correct in their domains"
        })).unwrap()).expect("conclude");
    }

    let state = StorageRead::read_state(&sched.bb);
    assert!(
        state.facts.len() > 9,
        "Knowledge grew through peer review: {} facts",
        state.facts.len()
    );
}

// ═════════════════════════════════════════════════════════════════════════
//  Scenario 3: Incremental Knowledge Growth (Many Iterations)
//  Detectors produce facts → agents read → create intents → conclude →
//  new facts → more detector facts → recursive growth stabilizes
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_incremental_knowledge_growth() {
    let bb = create_blackboard();

    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));
    sched.register(Box::new(ContradictionDetector::new()));
    sched.register(Box::new(StateChangeDetector::new()));

    // Phase 1: Silent init at 0 facts
    let _ = do_tick(&mut sched);

    // Seed AFTER silent init so state_change detects 0→11
    seed_cross_domain(&mut sched.bb);

    // Phase 2: First real tick — all detectors fire
    let r2 = do_tick(&mut sched);
    let gap_2 = count_by_creator(&r2.state, "gap-detector");
    let contradiction_2 = count_by_type(&r2.state, "contradiction");
    let state_change_2 = count_by_type(&r2.state, "state_change");
    assert!(gap_2 > 0, "Phase 2: gaps={}", gap_2);
    assert!(
        contradiction_2 > 0,
        "Phase 2: contradictions={}",
        contradiction_2
    );
    assert!(
        state_change_2 > 0,
        "Phase 2: state_changes={}",
        state_change_2
    );

    // Phase 3: Second tick — detectors don't re-analyze their own output
    let r3 = do_tick(&mut sched);
    let gap_3 = count_by_creator(&r3.state, "gap-detector");
    let contradiction_3 = count_by_type(&r3.state, "contradiction");
    assert_eq!(gap_3, gap_2, "Phase 3: gap facts stable (no re-analysis)");
    assert_eq!(
        contradiction_3, contradiction_2,
        "Phase 3: contradiction facts stable"
    );

    // Phase 4-7: Agent loop — read contradiction facts, create intents, conclude
    for iteration in 4..8 {
        let state = StorageRead::read_state(&sched.bb);
        let unresolved: Vec<_> = state
            .facts
            .iter()
            .filter(|f| f.creator == "contradiction-detector")
            .filter(|f| {
                // Check if any intent already references this contradiction fact
                !state.intents.iter().any(|i| i.from_facts.contains(&f.id))
            })
            .collect();

        if unresolved.is_empty() {
            break;
        }

        for cf in &unresolved {
            let content_json = content_val_of(cf);
            let topic = content_json
                .get("topic")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let intent = Intent {
                id: FihHash::new(
                    &[&cf.id.to_string(), &format!("iter-{}", iteration)],
                    "intent",
                ),
                from_facts: vec![cf.id.clone()],
                description: format!("Iteration {}: resolve {}", iteration, topic),
                creator: "agent-loop".into(),
                worker: None,
                to_fact_id: None,
                last_heartbeat_at: None,
                created_at: None,
                is_concluded: false,
                concluded_at: None,
            };
            let iid = sched.bb.submit_intent(&intent).expect("submit");
            sched.bb.claim_intent(&iid.to_string(), "agent-loop").expect("claim");
            sched
                .bb
                .conclude_intent(
                    &iid.to_string(),
                    &serde_json::to_string(&serde_json::json!({
                        "resolution": format!("Resolved {} in iteration {}", topic, iteration),
                        "iteration": iteration,
                    }))
                    .unwrap_or_default(),
                )
                .expect("conclude");
        }

        let _ = do_tick(&mut sched);
    }

    let final_state = StorageRead::read_state(&sched.bb);
    let total_facts = final_state.facts.len();
    let total_intents = final_state.intents.len();
    // Original 11 + detector facts + conclusions from iterations
    assert!(
        total_facts > 14,
        "Knowledge grew across iterations: {} facts, {} intents",
        total_facts,
        total_intents
    );

    // Fact types should include all 3 detector types
    assert!(count_by_type(&final_state, "gap") > 0, "Gap facts persist");
    assert!(
        count_by_type(&final_state, "contradiction") > 0,
        "Contradiction facts persist"
    );
    assert!(
        count_by_type(&final_state, "state_change") > 0,
        "State change facts persist"
    );
}

// ═════════════════════════════════════════════════════════════════════════
//  Scenario 4: Multi-Agent Collaboration
//  Different agents work on different aspects of the same corpus
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_multi_agent_collaboration() {
    let mut bb = create_blackboard();
    seed_cross_domain(&mut bb);

    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));
    sched.register(Box::new(ContradictionDetector::new()));

    // Run detectors once to produce analysis facts
    let r1 = do_tick(&mut sched);

    // Agent Alpha: Hardware specialist — looks at spatz-related gaps
    let gap_facts: Vec<_> = r1
        .state
        .facts
        .iter()
        .filter(|f| f.creator == "gap-detector" && f.content.to_string().contains("spatz"))
        .collect();
    for gf in &gap_facts {
        let intent = Intent {
            id: FihHash::new(&[&gf.id.to_string(), "alpha"], "intent"),
            from_facts: vec![gf.id.clone()],
            description: "Hardware gap analysis".into(),
            creator: "agent-alpha".into(),
            worker: None,
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            is_concluded: false,
            concluded_at: None,
        };
        let iid = sched.bb.submit_intent(&intent).expect("submit");
        sched.bb.claim_intent(&iid.to_string(), "agent-alpha").expect("claim");
        sched
            .bb
            .conclude_intent(
                &iid.to_string(),
                &serde_json::to_string(&serde_json::json!({
                    "analysis": "Spatz balance condition validates SSCCS structural model",
                    "domain": "hardware",
                    "agent": "alpha",
                }))
                .unwrap_or_default(),
            )
            .expect("conclude");
    }

    // Agent Beta: Compiler specialist — looks at eurollvm-related gaps
    let compiler_gaps: Vec<_> = r1
        .state
        .facts
        .iter()
        .filter(|f| f.creator == "gap-detector" && f.content.to_string().contains("eurollvm"))
        .collect();
    for gf in &compiler_gaps {
        let intent = Intent {
            id: FihHash::new(&[&gf.id.to_string(), "beta"], "intent"),
            from_facts: vec![gf.id.clone()],
            description: "Compiler gap analysis".into(),
            creator: "agent-beta".into(),
            worker: None,
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            is_concluded: false,
            concluded_at: None,
        };
        let iid = sched.bb.submit_intent(&intent).expect("submit");
        sched.bb.claim_intent(&iid.to_string(), "agent-beta").expect("claim");
        sched
            .bb
            .conclude_intent(
                &iid.to_string(),
                &serde_json::to_string(&serde_json::json!({
                    "analysis": "MLIR Transform Dialect aligns with SSCCS Field composition",
                    "domain": "compiler",
                    "agent": "beta",
                }))
                .unwrap_or_default(),
            )
            .expect("conclude");
    }

    // Agent Gamma: Philosopher — resolves contradictions
    let contradiction_facts: Vec<_> = r1
        .state
        .facts
        .iter()
        .filter(|f| f.creator == "contradiction-detector")
        .collect();
    for cf in &contradiction_facts {
        let intent = Intent {
            id: FihHash::new(&[&cf.id.to_string(), "gamma"], "intent"),
            from_facts: vec![cf.id.clone()],
            description: "Philosophical resolution".into(),
            creator: "agent-gamma".into(),
            worker: None,
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            is_concluded: false,
            concluded_at: None,
        };
        let iid = sched.bb.submit_intent(&intent).expect("submit");
        sched.bb.claim_intent(&iid.to_string(), "agent-gamma").expect("claim");
        sched.bb.conclude_intent(&iid.to_string(), &serde_json::to_string(&serde_json::json!({
            "synthesis": "SSCCS theory + Spatz measurement + MLIR implementation are complementary layers",
            "domain": "philosophy",
            "agent": "gamma",
        })).unwrap()).expect("conclude");
    }

    let state = StorageRead::read_state(&sched.bb);
    // Verify each agent's conclusions exist
    let alpha_conclusions = state
        .facts
        .iter()
        .filter(|f| content_val_of(f).get("agent").and_then(|v| v.as_str()) == Some("alpha"))
        .count();
    let beta_conclusions = state
        .facts
        .iter()
        .filter(|f| content_val_of(f).get("agent").and_then(|v| v.as_str()) == Some("beta"))
        .count();
    let gamma_conclusions = state
        .facts
        .iter()
        .filter(|f| content_val_of(f).get("agent").and_then(|v| v.as_str()) == Some("gamma"))
        .count();

    assert!(alpha_conclusions > 0, "Agent Alpha (hardware) contributed");
    assert!(beta_conclusions > 0, "Agent Beta (compiler) contributed");
    assert!(
        gamma_conclusions > 0,
        "Agent Gamma (philosophy) contributed"
    );

    // No direct agent-to-agent communication — all through Blackboard
    let total = alpha_conclusions + beta_conclusions + gamma_conclusions;
    assert!(
        total >= 3,
        "3 agents, {} conclusions — stigmergy in action",
        total
    );
}

// ═════════════════════════════════════════════════════════════════════════
//  Scenario 5: Document Revision
//  v1 facts → detector analysis → v2 arrives with changes → state change
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_document_revision() {
    let bb = create_blackboard();

    // v1: Initial document claims
    let v1 = [
        claim(
            "v1-01",
            "design-doc-v1.llms.md",
            "SSCCS compiler uses Rust for frontend parsing",
            "compiler-implementation",
            "rust-only",
        ),
        claim(
            "v1-02",
            "design-doc-v1.llms.md",
            "Memory layout is resolved at compile time via declarative mapping",
            "memory-model",
            "compile-time",
        ),
        claim(
            "v1-03",
            "design-doc-v1.llms.md",
            "Observation is a purely software operation on current hardware",
            "observation-scope",
            "software-only",
        ),
    ];
    let v1_ids: Vec<String> = v1.iter().map(|f| f.id.to_string()).collect();

    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));
    sched.register(Box::new(StateChangeDetector::new()));

    // Phase 1: Silent init at 0 facts
    let _ = do_tick(&mut sched);

    // Seed v1 AFTER silent init
    for f in &v1 {
        sched.bb.submit_fact(f).unwrap();
    }
    sched.register(Box::new(NewDocumentAnalyzer::with_baseline(v1_ids)));

    // Phase 1b: v1 ingestion detected
    let _r1 = do_tick(&mut sched); // state_change detects 0→3 facts
    let r2 = do_tick(&mut sched); // gap detection on 3 facts (may be too few for gaps)

    let state_change_v1 = count_by_type(&r2.state, "state_change");
    assert!(
        state_change_v1 > 0,
        "v1 ingestion detected: {} state_changes",
        state_change_v1
    );

    // Phase 2: v2 arrives — revised claims
    let v2 = [
        claim(
            "v2-01",
            "design-doc-v2.llms.md",
            "SSCCS compiler uses Rust + MLIR hybrid via Melior",
            "compiler-implementation",
            "rust-mlir-hybrid",
        ),
        claim(
            "v2-02",
            "design-doc-v2.llms.md",
            "Memory layout is resolved at compile time, with runtime feedback loop",
            "memory-model",
            "compile-time-with-feedback",
        ),
        claim(
            "v2-03",
            "design-doc-v2.llms.md",
            "Observation targets both software emulation and PIM hardware",
            "observation-scope",
            "software-and-hardware",
        ),
    ];
    for f in &v2 {
        sched.bb.submit_fact(f).unwrap();
    }

    let r3 = do_tick(&mut sched);

    // NDA: should find -factors (v2 challenges v1 positions) and gaps (new topics from v2 origins)
    let nda_minus: Vec<_> = r3
        .state
        .facts
        .iter()
        .filter(|f| {
            f.creator == "new-document-analyzer"
                && content_val_of(f).get("factor").and_then(|v| v.as_str()) == Some("-factor")
        })
        .collect();

    // Every v2 topic should be a -factor — v2 has different positions for all topics
    assert!(
        nda_minus.len() >= 3,
        "v2 challenges all v1 positions: {} -factors",
        nda_minus.len()
    );

    // State change: facts 3→6
    let sc_after_v2 = count_by_type(&r3.state, "state_change");
    assert!(
        sc_after_v2 > state_change_v1,
        "More state changes after v2: {} (was {})",
        sc_after_v2,
        state_change_v1
    );

    // Agent: decide to adopt v2, deprecate v1
    let v2_facts: Vec<_> = r3
        .state
        .facts
        .iter()
        .filter(|f| f.origin == "design-doc-v2.llms.md")
        .collect();
    assert_eq!(v2_facts.len(), 3, "All 3 v2 facts present");

    // Snapshot the evolved state
    let snapshot = Snapshottable::to_snapshot(&sched.bb);
    let json = serde_json::to_vec(&snapshot).expect("serialize");
    let restored: StorageSnapshot = serde_json::from_slice(&json).expect("deserialize");
    let bb_restored = <HybridBlackboard as Snapshottable>::from_snapshot(restored);
    let restored_state = StorageRead::read_state(&bb_restored);
    assert!(
        restored_state.facts.len() >= 6,
        "Snapshot preserves v1 + v2 + detector facts: {}",
        restored_state.facts.len()
    );
}
