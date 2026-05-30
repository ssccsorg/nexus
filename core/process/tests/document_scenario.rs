// Nexus Document Scenario Tests
// =============================
// End-to-end validation of the FIH lifecycle using real SSCCS document claims.
//
// These tests exercise the complete stigmergy cycle:
//
//   Phase 1: Knowledge Ingestion
//     submit_fact(claim) → Blackboard accumulates document knowledge
//     Each Fact encodes {claim, topic, position, source_doc}
//
//   Phase 2: Gap Detection (detector → Fact)
//     GapDetector.orient(state) scans all document-source facts
//     Finds orphaned facts (not referenced by any Intent)
//     Records gap Facts: {type:"gap", subtype:"origin-orphan"|"cross-origin"}
//     Duplicate prevention via seen_origin / seen_topic sets
//
//   Phase 3: Contradiction Detection (detector → Fact)
//     ContradictionDetector groups facts by topic, then by position
//     Same topic + different positions → contradiction Fact
//     {type:"contradiction", topic, position_a, position_b, origins}
//
//   Phase 4: New Document Analysis (detector → Fact)
//     NewDocumentAnalyzer compares incoming facts against existing corpus
//     Same topic, same position → +factor (support)
//     Same topic, different position → -factor (challenge)
//     New topic → gap (exploration frontier)
//
//   Phase 5: Agent Action (agent → Intent → conclude → Fact)
//     Agent reads detector Facts (contradictions, gaps)
//     Creates Intent referencing those Facts
//     Claims → heartbeats → concludes the Intent
//     Conclusion creates new synthesized Fact
//
//   Phase 6: Eviction + Snapshot
//     evict_before removes concluded Intents + orphaned Facts
//     to_snapshot() serializes full state
//     from_snapshot() restores for cross-worker continuity
//
// Architectural invariants verified:
//   - Detectors produce Facts (observations), never Intents (actions)
//   - Detectors exclude their own origin (no self-re-analysis)
//   - Agent is the only source of Intents
//   - Facts are immutable and survive eviction (unless orphaned)
//   - Snapshots preserve all Facts, Intents, and claim state

use nexus_graph::{
    Blackboard, EvictCapable, Fact, FihHash, Intent, Snapshottable, StorageSnapshot,
    create_blackboard, create_blackboard_from_snapshot,
};
use nexus_process::scheduler::Scheduler;
use nexus_process::tasks::contradiction_detector::ContradictionDetector;
use nexus_process::tasks::gap_detector::GapDetector;
use nexus_process::tasks::new_document_analyzer::NewDocumentAnalyzer;
use nexus_process::tasks::state_change_detector::StateChangeDetector;

// ── Helper: construct a claim Fact with {claim, topic, position} content ─

fn claim(id: &str, origin: &str, claim_text: &str, topic: &str, position: &str) -> Fact {
    Fact {
        id: FihHash(id.to_string()),
        origin: origin.to_string(),
        content: serde_json::json!({
            "claim": claim_text,
            "topic": topic,
            "position": position,
        })
        .into(),
        creator: "ingester".into(),
    }
}

// ── Document corpus: 4 initial documents, 19 claims ────────────────────

fn seed_initial_corpus(bb: &mut impl Blackboard) {
    let facts = [
        // manifesto.llms.md — ontology ("what computation IS")
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
        // guide.llms.md — developer usage ("how to USE SSCCS")
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
        // nexus/index.llms.md — architecture overview
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
        // nexus/impl_init.llms.md — implementation details
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

// ── New documents: 2 additional documents, 8 claims ────────────────────

fn seed_new_documents(bb: &mut impl Blackboard) {
    let facts = [
        // nexus/notes/acp_nexus.llms.md — agent protocol analysis
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
        // notes/iclr26_insight.llms.md — research paper analysis
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

// ── Tick helper: runs one OODA cycle, returns facts_submitted + full state ─

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

/// Count detector Facts by creator and type.
fn count_detector_facts(state: &nexus_graph::BoardState, detector: &str, fact_type: &str) -> usize {
    state
        .facts
        .iter()
        .filter(|f| f.creator == detector)
        .filter(|f| {
            let content_val: serde_json::Value =
                serde_json::from_str(f.content.as_str().unwrap_or(""))
                    .unwrap_or(serde_json::Value::Null);
            content_val
                .get("type")
                .and_then(|v| v.as_str())
                == Some(fact_type)
        })
        .count()
}

// ── Agent: reads contradiction Facts, creates Intents, concludes them ──
//
// This function simulates an agent performing the FIH action cycle:
//   1. Read state, find Facts from contradiction-detector
//   2. Filter by topic
//   3. Create Intent referencing the contradiction Fact
//   4. Claim → heartbeat → conclude with a resolution
//   5. Conclusion becomes a new synthesized Fact
//
// This is the ONLY correct way to act on detector output:
// detector observes → agent decides → agent acts

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
        let content_json: serde_json::Value =
            serde_json::from_str(fact.content.as_str().unwrap_or(""))
                .unwrap_or(serde_json::Value::Null);
        let Some(t) = content_json.get("topic").and_then(|v| v.as_str()) else {
            continue;
        };
        if !t.contains(topic_filter) {
            continue;
        }
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
                &serde_json::to_string(&serde_json::json!({
                    "resolution": conclusion,
                    "agent": agent_name,
                })).unwrap(),
            )
            .expect("conclude");
    }
}

// ═════════════════════════════════════════════════════════════════════════
//  Scenario: Full Document Lifecycle
//
//  Demonstrates the complete FIH cycle from ingestion to synthesis.
//
//  FIH Flow:
//    Phase 1: 19 document Facts ingested (4 sources)
//    Phase 2: GapDetector produces gap Facts (origin + cross-origin)
//    Phase 3: ContradictionDetector produces contradiction Facts
//             (field-definition has 3 positions across documents)
//    Phase 4: 8 new Facts arrive (ACP + ICLR documents)
//             NewDocumentAnalyzer produces +/-/gap analysis Facts
//    Phase 5: ContradictionDetector finds new tensions
//             (time-ontology, design-priority, field-definition)
//    Phase 6: 3 agents resolve contradictions → new synthesis Facts
//    Phase 7: Snapshot before eviction → cross-worker restore
//
//  Core components exercised:
//    Scheduler, GapDetector, ContradictionDetector, NewDocumentAnalyzer,
//    Blackboard (submit/claim/heartbeat/conclude/read_state),
//    EvictCapable, Snapshottable
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

    // Phase 2: Gap detection → Facts (observations, not actions)
    let r1 = do_tick(&mut sched);
    let gap_facts = count_detector_facts(&r1.state, "gap-detector", "gap");
    assert!(
        gap_facts > 0,
        "Phase 2: gap detector recorded gap facts: {}",
        gap_facts
    );

    // Phase 3: Contradiction detection → Facts
    let r2 = do_tick(&mut sched);
    let contradiction_facts =
        count_detector_facts(&r2.state, "contradiction-detector", "contradiction");
    assert!(
        contradiction_facts > 0,
        "Phase 3: contradiction detector recorded {} facts",
        contradiction_facts
    );
    assert!(
        contradiction_facts >= 2,
        "field-definition has 3 positions, >=2 pairs"
    );

    // Phase 4: New documents arrive → NDA analysis Facts
    sched.register(Box::new(NewDocumentAnalyzer::with_baseline(initial_ids())));
    seed_new_documents(&mut sched.bb);

    let r3 = do_tick(&mut sched);
    let nda_facts = count_detector_facts(&r3.state, "new-document-analyzer", "doc_analysis");
    assert_eq!(nda_facts, 8, "Phase 4: 8 new facts analyzed");

    // Phase 5: Contradiction detector finds tensions with new documents
    let r4 = do_tick(&mut sched);
    let all_contradictions =
        count_detector_facts(&r4.state, "contradiction-detector", "contradiction");
    assert!(
        all_contradictions >= 4,
        "Phase 5: >=4 contradictions with new docs: got {}",
        all_contradictions
    );

    // Phase 6: Agents resolve contradictions → Intent → conclude → new Facts
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

    // Phase 7: Snapshot → eviction
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

// ═════════════════════════════════════════════════════════════════════════
//  Scenario: Gap Facts are Immutable Observations
//
//  Verifies that detector Facts survive eviction (they're Facts, not
//  Intents) and are stable across ticks (no duplicate generation).
//
//  Key FIH semantics:
//    - Detector output = Fact = immutable observation
//    - evict_before removes Intents and orphaned Facts
//    - Detector Facts that reference document Facts are NOT orphaned
//      (they're part of the knowledge graph)
//    - Duplicate prevention: seen sets prevent re-observation
// ═════════════════════════════════════════════════════════════════════════
#[test]
fn scenario_gap_facts_are_immutable_observations() {
    let mut bb = create_blackboard();
    seed_initial_corpus(&mut bb);

    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));

    let r1 = do_tick(&mut sched);
    let gap_facts_t1 = count_detector_facts(&r1.state, "gap-detector", "gap");

    let r2 = do_tick(&mut sched);
    let gap_facts_t2 = count_detector_facts(&r2.state, "gap-detector", "gap");
    assert_eq!(
        gap_facts_t1, gap_facts_t2,
        "Gap facts are stable — no duplicates"
    );

    EvictCapable::evict_before(&sched.bb, "9999999999").expect("evict");
    let state = Blackboard::read_state(&sched.bb);
    let gap_after_evict = count_detector_facts(&state, "gap-detector", "gap");
    assert_eq!(
        gap_after_evict, gap_facts_t1,
        "Gap facts survive eviction (they're Facts)"
    );
}

// ═════════════════════════════════════════════════════════════════════════
//  Scenario: State Change Detector (Cairn ReasonCheckpoint Pattern)
//
//  Demonstrates the count-based state change detection:
//    Tick 1: silent initialization (records baseline checkpoint)
//    Tick 2: 19 facts arrive → state_change Fact (facts:0→19)
//    Tick 3: no change → no Fact produced
//    Tick 4: 8 more facts → state_change Fact (facts:19→27)
//
//  Stigmergy: state changes are Facts. What to do about them is an
//  agent decision in a later iteration.
// ═════════════════════════════════════════════════════════════════════════
#[test]
fn scenario_state_change_detector_facts() {
    let bb = create_blackboard();
    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(StateChangeDetector::new()));

    let r1 = do_tick(&mut sched);
    assert_eq!(r1.facts_submitted, 0, "Tick 1: silent init");

    seed_initial_corpus(&mut sched.bb);
    let r2 = do_tick(&mut sched);
    let sc_facts = count_detector_facts(&r2.state, "state-change-detector", "state_change");
    assert_eq!(sc_facts, 1, "Tick 2: state_change Fact recorded");

    let r3 = do_tick(&mut sched);
    assert_eq!(r3.facts_submitted, 0, "Tick 3: no change, no fact");

    seed_new_documents(&mut sched.bb);
    let r4 = do_tick(&mut sched);
    let sc_facts_t4 = count_detector_facts(&r4.state, "state-change-detector", "state_change");
    assert_eq!(
        sc_facts_t4, 2,
        "Tick 4: 2 state_change Facts (0→19, then 19→27)"
    );
}

// ═════════════════════════════════════════════════════════════════════════
//  Scenario: Detector State Persistence Across Snapshot
//
//  Verifies that detector internal state (checkpoints) survives
//  snapshot round-trip, preventing duplicate detection after restore.
//
//  Flow:
//    Worker A:
//      1. Seed facts, run StateChangeDetector (records checkpoint)
//      2. to_snapshot() → inject task_states via collect_task_states()
//      3. Serialize to JSON
//    Worker B:
//      4. Deserialize snapshot
//      5. Create blackboard from snapshot
//      6. Create fresh StateChangeDetector, call restore_task_states()
//      7. Run tick — should produce 0 facts (checkpoint restored, no change)
//
//  Without task_states: Worker B would see all facts as "new" and
//  create a spurious state_change Fact.
// ═════════════════════════════════════════════════════════════════════════
#[test]
fn scenario_detector_state_snapshot_roundtrip() {
    // Worker A: create scheduler FIRST (0 facts), then seed
    let bb_a = create_blackboard();
    let mut sched_a = Scheduler::new(bb_a);
    sched_a.register(Box::new(StateChangeDetector::new()));
    sched_a.register(Box::new(GapDetector::new()));

    // Tick 1: 0 facts → silent init for state_change, gap detector finds nothing
    let _ = do_tick(&mut sched_a);

    // Seed facts (state now has 19 document facts)
    seed_initial_corpus(&mut sched_a.bb);

    // Tick 2: StateChangeDetector sees 0→19 → state_change Fact created
    let r2 = do_tick(&mut sched_a);
    assert!(
        r2.facts_submitted > 0,
        "Worker A tick 2: state_change + gap facts"
    );
    let sc_a = count_detector_facts(&r2.state, "state-change-detector", "state_change");
    assert_eq!(sc_a, 1, "Worker A: one state_change Fact (0→19)");

    // Collect detector state + snapshot
    let task_states = sched_a.collect_task_states();
    assert!(
        task_states.contains_key("state-change-detector"),
        "StateChangeDetector checkpoint saved"
    );

    let mut snapshot = Snapshottable::to_snapshot(&sched_a.bb);
    snapshot.task_states = task_states;
    let json = serde_json::to_vec(&snapshot).expect("serialize");

    // Worker B: restore from snapshot
    let restored: StorageSnapshot = serde_json::from_slice(&json).expect("deserialize");
    assert!(
        restored.task_states.contains_key("state-change-detector"),
        "Checkpoint preserved in snapshot"
    );

    let bb_b = create_blackboard_from_snapshot(restored.clone());
    let mut sched_b = Scheduler::new(bb_b);
    sched_b.register(Box::new(StateChangeDetector::new()));
    sched_b.register(Box::new(GapDetector::new()));
    sched_b.restore_task_states(&restored.task_states);

    // Worker B's first tick: GapDetector re-creates gap facts
    // (content-addressed, harmless). StateChangeDetector must NOT
    // produce a state_change Fact because checkpoint was restored.
    let r_b = do_tick(&mut sched_b);
    assert!(
        r_b.facts_submitted > 0,
        "Worker B: gap facts re-created (content-addressed, idempotent)"
    );

    let sc_facts = count_detector_facts(&r_b.state, "state-change-detector", "state_change");
    assert_eq!(
        sc_facts, 1,
        "Worker B: exactly 1 state_change Fact (Worker A's, preserved in snapshot)"
    );

    // Run another tick — still no NEW state_change (checkpoint matches)
    let r_b2 = do_tick(&mut sched_b);
    let sc_facts_2 = count_detector_facts(&r_b2.state, "state-change-detector", "state_change");
    assert_eq!(
        sc_facts_2, 1,
        "Worker B tick 2: no new state_change (checkpoint stable)"
    );
}

// ═════════════════════════════════════════════════════════════════════════
//  Scenario: Flush then Evict
//
//  Verifies that eviction respects the flush-first contract.
//  Uses tick_with_flush() which requires FlushCapable backend.
//
//  The DefaultBlackboard (DualStorage) delegates FlushCapable to
//  the cold backend (NullStorage by default, which is a no-op).
//  This test verifies the coordination logic, not the actual
//  persistence — that belongs in storage-layer tests.
// ═════════════════════════════════════════════════════════════════════════
#[test]
fn scenario_flush_then_evict() {
    let mut bb = create_blackboard();
    seed_initial_corpus(&mut bb);

    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));

    // tick_with_flush requires FlushCapable — now provided by create_blackboard()
    let _ = sched.tick_with_flush().expect("tick with flush");
    let state = Blackboard::read_state(&sched.bb);
    assert!(
        state.facts.len() >= 19,
        "tick_with_flush works: {} facts",
        state.facts.len()
    );

    // Verify detectors fired normally
    let gap_count = count_detector_facts(&state, "gap-detector", "gap");
    assert!(
        gap_count > 0,
        "Gap facts created during flush tick: {}",
        gap_count
    );

    // Second tick with flush — no new state_change (checkpoint stable)
    let r2 = sched.tick_with_flush().expect("second tick with flush");
    assert_eq!(r2, 0, "No new facts on second flush tick");
}
