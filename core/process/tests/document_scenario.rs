// Document-level scenario test: validates the full Nexus pipeline with
// real SSCCS document claims.
//
// Refactored for correct FIH semantics:
//   - Detectors produce Facts (observations), never Intents (actions)
//   - Agents read detector Facts and create Intents to act on them
//   - Intents go through claim → heartbeat → conclude lifecycle
//   - Concluded Intents produce new Facts → recursive knowledge growth
//
// Stigmergy principle: dumb detectors, infinite iterations, emergent accuracy.

use nexus_graph::{
    Blackboard, EvictCapable, Fact, FihHash, Intent, Snapshottable, StorageSnapshot,
    create_blackboard, create_blackboard_from_snapshot,
};
use nexus_process::scheduler::Scheduler;
use nexus_process::tasks::contradiction_detector::ContradictionDetector;
use nexus_process::tasks::gap_detector::GapDetector;
use nexus_process::tasks::new_document_analyzer::NewDocumentAnalyzer;
use nexus_process::tasks::state_change_detector::StateChangeDetector;

// ── Helper: construct a claim Fact ──────────────────────────────────────

fn claim(id: &str, origin: &str, claim_text: &str, topic: &str, position: &str) -> Fact {
    Fact {
        id: FihHash(id.to_string()),
        origin: origin.to_string(),
        content: serde_json::json!({
            "claim": claim_text,
            "topic": topic,
            "position": position,
        }),
        creator: "ingester".into(),
    }
}

// ── Seed data ───────────────────────────────────────────────────────────

fn seed_initial_corpus(bb: &mut impl Blackboard) {
    let facts = [
        // manifesto.llms.md
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
            "Time is just another coordinate, not a privileged timeline",
            "time-ontology",
            "coordinate-only",
        ),
        claim(
            "m03",
            "manifesto.llms.md",
            "Field does not store values; it stores admissibility conditions",
            "field-definition",
            "abstract-constraint",
        ),
        claim(
            "m04",
            "manifesto.llms.md",
            "There is no instruction stream",
            "execution-model",
            "collapse-based",
        ),
        claim(
            "m05",
            "manifesto.llms.md",
            "Segments are immutable, stateless coordinate points",
            "segment-property",
            "immutable-coordinate",
        ),
        claim(
            "m06",
            "manifesto.llms.md",
            "Performance is not the first objective; structural fidelity is",
            "design-priority",
            "fidelity-first",
        ),
        // guide.llms.md
        claim(
            "g01",
            "guide.llms.md",
            "Field is a set of dynamic rules — the only mutable layer",
            "field-definition",
            "practical-mutable",
        ),
        claim(
            "g02",
            "guide.llms.md",
            "Segment is a pure coordinate point",
            "segment-property",
            "coordinate-only",
        ),
        claim(
            "g03",
            "guide.llms.md",
            "A developer designs structure and sets conditions",
            "developer-role",
            "structure-designer",
        ),
        claim(
            "g04",
            "guide.llms.md",
            "Field gives meaning to points: point 0 is 1, point 1 is 1",
            "field-definition",
            "value-binding",
        ),
        claim(
            "g05",
            "guide.llms.md",
            "Data movement consumes 60-80% of energy",
            "energy-efficiency",
            "data-movement-cost",
        ),
        // nexus/index.llms.md
        claim(
            "n01",
            "nexus/index.llms.md",
            "FIH primitives are the only interface between agents",
            "agent-interface",
            "fih-only",
        ),
        claim(
            "n02",
            "nexus/index.llms.md",
            "No fixed pipeline, no direct agent-to-agent communication",
            "agent-coordination",
            "stigmergy-indirect",
        ),
        claim(
            "n03",
            "nexus/index.llms.md",
            "Stigmergy: agents leave traces, others perceive and adapt",
            "agent-coordination",
            "stigmergy-indirect",
        ),
        claim(
            "n04",
            "nexus/index.llms.md",
            "Five layers: KG Engine, Ingestion, Research Loop, Learning, Governance",
            "architecture-layers",
            "five-layer",
        ),
        // nexus/impl_init.llms.md
        claim(
            "i01",
            "nexus/impl_init.llms.md",
            "Core assembled from production-grade crates",
            "architecture-approach",
            "crate-assembly",
        ),
        claim(
            "i02",
            "nexus/impl_init.llms.md",
            "Single codebase compiles to WASM and native",
            "deployment-model",
            "unified-binary",
        ),
        claim(
            "i03",
            "nexus/impl_init.llms.md",
            "Cypher has richest LLM training data",
            "query-language",
            "cypher-first",
        ),
        claim(
            "i04",
            "nexus/impl_init.llms.md",
            "LLMs are accelerators, not requirements",
            "llm-role",
            "optional-accelerator",
        ),
    ];
    for f in &facts {
        bb.submit_fact(f).unwrap();
    }
}

fn initial_ids() -> Vec<String> {
    [
        "m01", "m02", "m03", "m04", "m05", "m06", "g01", "g02", "g03", "g04", "g05", "n01", "n02",
        "n03", "n04", "i01", "i02", "i03", "i04",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn seed_new_documents(bb: &mut impl Blackboard) {
    let facts = [
        claim(
            "a01",
            "nexus/notes/acp_nexus.llms.md",
            "ACP uses subprocess model with stdio transport",
            "agent-architecture",
            "subprocess-isolation",
        ),
        claim(
            "a02",
            "nexus/notes/acp_nexus.llms.md",
            "MCP/ACP/A2A form a three-layer protocol stack",
            "protocol-stack",
            "layered-protocols",
        ),
        claim(
            "a03",
            "nexus/notes/acp_nexus.llms.md",
            "Provider independence is a correctness requirement",
            "llm-backend",
            "provider-agnostic",
        ),
        claim(
            "a04",
            "nexus/notes/acp_nexus.llms.md",
            "Multi-backend validation ensures cross-model comparison",
            "llm-backend",
            "provider-agnostic",
        ),
        claim(
            "c01",
            "notes/iclr26_insight.llms.md",
            "Mamba-3: inference-first with time-aware discretization",
            "time-ontology",
            "time-aware-optimization",
        ),
        claim(
            "c02",
            "notes/iclr26_insight.llms.md",
            "Add arithmetic intensity check to MemoryLayout",
            "design-priority",
            "performance-first",
        ),
        claim(
            "c03",
            "notes/iclr26_insight.llms.md",
            "Diagonal SSMs cannot express non-Abelian state tracking",
            "expressivity-limits",
            "provable-ceiling",
        ),
        claim(
            "c04",
            "notes/iclr26_insight.llms.md",
            "Replace complexity matrix with roofline analysis",
            "design-priority",
            "performance-first",
        ),
    ];
    for f in &facts {
        bb.submit_fact(f).unwrap();
    }
}

// ── Tick helper ────────────────────────────────────────────────────────

struct TickResult {
    facts_submitted: usize,
    state: nexus_graph::BoardState,
}

fn do_tick(sched: &mut Scheduler<impl Blackboard + EvictCapable + Snapshottable>) -> TickResult {
    let facts_submitted = sched.tick().expect("tick");
    let state = Blackboard::read_state(&sched.bb);
    TickResult {
        facts_submitted,
        state,
    }
}

/// Count detector Facts of a given type (using content["type"]).
fn count_detector_facts(state: &nexus_graph::BoardState, detector: &str, fact_type: &str) -> usize {
    state
        .facts
        .iter()
        .filter(|f| f.creator == detector)
        .filter(|f| f.content.get("type").and_then(|v| v.as_str()) == Some(fact_type))
        .count()
}

// ── Agent: create Intents from contradiction Facts ─────────────────────

fn agent_resolve_contradictions(
    sched: &mut Scheduler<impl Blackboard + EvictCapable + Snapshottable>,
    agent_name: &str,
    topic_filter: &str,
    conclusion: &str,
) {
    let state = Blackboard::read_state(&sched.bb);
    for fact in &state.facts {
        if fact.creator != "contradiction-detector" {
            continue;
        }
        let Some(t) = fact.content.get("topic").and_then(|v| v.as_str()) else {
            continue;
        };
        if !t.contains(topic_filter) {
            continue;
        }
        // Create an Intent to resolve this contradiction
        let intent = Intent {
            id: FihHash::new(&[&fact.id.0, agent_name], "resolve"),
            from_facts: vec![fact.id.0.clone()],
            description: format!("Resolve {}: {}", t, agent_name),
            creator: agent_name.into(),
            worker: None,
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            concluded_at: None,
        };
        let iid = sched.bb.submit_intent(&intent).expect("submit intent");
        sched.bb.claim_intent(&iid.0, agent_name).expect("claim");
        sched.bb.heartbeat(&iid.0, agent_name).expect("heartbeat");
        sched
            .bb
            .conclude_intent(
                &iid.0,
                &serde_json::json!({
                    "resolution": conclusion,
                    "agent": agent_name,
                }),
            )
            .expect("conclude");
    }
}

// ═════════════════════════════════════════════════════════════════════════
//  Scenario: Full document lifecycle with correct FIH semantics
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_full_document_lifecycle() {
    let mut bb = create_blackboard();
    seed_initial_corpus(&mut bb);

    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));
    sched.register(Box::new(ContradictionDetector::new()));

    let state = Blackboard::read_state(&sched.bb);
    assert_eq!(
        state.facts.len(),
        19,
        "Phase 1: 19 initial facts from 4 documents"
    );

    // ── Phase 2: Gap detection → Facts ─────────────────────────────
    let r1 = do_tick(&mut sched);
    let gap_facts = count_detector_facts(&r1.state, "gap-detector", "gap");
    assert!(
        gap_facts > 0,
        "Phase 2: gap detector recorded gap facts: {}",
        gap_facts
    );

    // ── Phase 3: Contradiction detection → Facts ───────────────────
    let r2 = do_tick(&mut sched);
    let contradiction_facts =
        count_detector_facts(&r2.state, "contradiction-detector", "contradiction");
    assert!(
        contradiction_facts > 0,
        "Phase 3: contradiction detector recorded {} facts",
        contradiction_facts
    );
    // field-definition has 3 positions → at least 2 contradiction pairs
    assert!(
        contradiction_facts >= 2,
        "field-definition has 3 positions, >=2 pairs"
    );

    // ── Phase 4: New documents arrive → NDA Facts ─────────────────
    sched.register(Box::new(NewDocumentAnalyzer::with_baseline(initial_ids())));
    seed_new_documents(&mut sched.bb);

    let r3 = do_tick(&mut sched);
    let nda_facts = count_detector_facts(&r3.state, "new-document-analyzer", "doc_analysis");
    assert_eq!(nda_facts, 8, "Phase 4: 8 new facts analyzed");

    // ── Phase 5: Contradiction detector finds new tensions ─────────
    let r4 = do_tick(&mut sched);
    let all_contradictions =
        count_detector_facts(&r4.state, "contradiction-detector", "contradiction");
    assert!(
        all_contradictions >= 4,
        "Phase 5: >=4 contradictions with new docs: got {}",
        all_contradictions
    );

    // ── Phase 6: Agent resolves contradictions ────────────────────
    // Agent reads contradiction Facts, creates Intents, concludes them
    agent_resolve_contradictions(
        &mut sched,
        "agent-alpha",
        "design-priority",
        "Structural fidelity is the primary constraint; roofline analysis is a secondary compiler concern",
    );
    agent_resolve_contradictions(
        &mut sched,
        "agent-beta",
        "time-ontology",
        "Time as coordinate is ontological truth; discretization is a Field-layer optimization",
    );
    agent_resolve_contradictions(
        &mut sched,
        "agent-gamma",
        "field-definition",
        "Abstract admissibility and value-binding are complementary views of Field",
    );

    let state = Blackboard::read_state(&sched.bb);
    let total_facts = state.facts.len();
    assert!(
        total_facts >= 28,
        "Phase 6: >=28 facts (19 initial + 8 new + detector + conclusions): got {}",
        total_facts
    );

    // ── Phase 7: Snapshot then Eviction ────────────────────────────
    // Snapshot BEFORE eviction to capture the full state
    let snapshot = Snapshottable::to_snapshot(&sched.bb);
    let json = serde_json::to_vec(&snapshot).expect("serialize");
    let restored: StorageSnapshot = serde_json::from_slice(&json).expect("deserialize");
    let bb_restored = create_blackboard_from_snapshot(restored);
    let state_restored = Blackboard::read_state(&bb_restored);
    assert_eq!(
        state_restored.facts.len(),
        total_facts,
        "Phase 7a: snapshot preserves all {} facts",
        total_facts
    );

    let state_before = Blackboard::read_state(&sched.bb);
    let intents_before = state_before.intents.len();
    EvictCapable::evict_before(&sched.bb, "9999999999").expect("evict");
    let state_after = Blackboard::read_state(&sched.bb);
    assert!(
        state_after.intents.len() < intents_before,
        "Phase 7b: concluded intents evicted"
    );
}

#[test]
fn scenario_gap_facts_are_immutable_observations() {
    let mut bb = create_blackboard();
    seed_initial_corpus(&mut bb);

    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));

    // Tick 1: gap Facts recorded
    let r1 = do_tick(&mut sched);
    let gap_facts_t1 = count_detector_facts(&r1.state, "gap-detector", "gap");

    // Tick 2: same data → no new gap Facts (already observed)
    let r2 = do_tick(&mut sched);
    let gap_facts_t2 = count_detector_facts(&r2.state, "gap-detector", "gap");
    assert_eq!(
        gap_facts_t1, gap_facts_t2,
        "Gap facts are stable — no duplicates"
    );

    // Gap facts persist (they're Facts, not evicted Intents)
    EvictCapable::evict_before(&sched.bb, "9999999999").expect("evict");
    let state = Blackboard::read_state(&sched.bb);
    let gap_after_evict = count_detector_facts(&state, "gap-detector", "gap");
    assert_eq!(
        gap_after_evict, gap_facts_t1,
        "Gap facts survive eviction (they're Facts)"
    );
}

#[test]
fn scenario_state_change_detector_facts() {
    let bb = create_blackboard();
    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(StateChangeDetector::new()));

    // Tick 1: silent init
    let r1 = do_tick(&mut sched);
    assert_eq!(r1.facts_submitted, 0, "Tick 1: silent init");

    // Seed + tick 2: fact count changed → state_change Fact
    seed_initial_corpus(&mut sched.bb);
    let r2 = do_tick(&mut sched);
    let sc_facts = count_detector_facts(&r2.state, "state-change-detector", "state_change");
    assert_eq!(sc_facts, 1, "Tick 2: state_change Fact recorded");

    // Tick 3: no change → no new Fact
    let r3 = do_tick(&mut sched);
    assert_eq!(r3.facts_submitted, 0, "Tick 3: no change, no fact");

    // Add new docs → Tick 4: another state_change Fact
    seed_new_documents(&mut sched.bb);
    let r4 = do_tick(&mut sched);
    let sc_facts_t4 = count_detector_facts(&r4.state, "state-change-detector", "state_change");
    // Tick 2: facts 0→19, Tick 4: facts 19→27
    assert_eq!(
        sc_facts_t4, 2,
        "Tick 4: 2 state_change Facts (facts 0→19, then 19→27)"
    );
}
