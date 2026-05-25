// Document-level scenario test: validates the full Nexus pipeline with
// real SSCCS document claims. This is the practical demonstration of:
//
//   - Multi-document knowledge ingestion
//   - Cross-origin gap detection
//   - Contradiction discovery between documents
//   - New document analysis (+/-/gap factors)
//   - Parallel hypothesis branches via stigmergy
//   - Eviction + snapshot round-trip
//
// The test uses actual claims from docs.ssccs.org documents encoded as
// Facts with {claim, topic, position} metadata.

use nexus_graph::{
    Blackboard, EvictCapable, Fact, FihHash, Snapshottable, StorageSnapshot, create_blackboard,
    create_blackboard_from_snapshot,
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

// ── Phase 1: Seed initial corpus (4 documents, 19 facts) ───────────────

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
            "Time is just another coordinate, not a privileged timeline of execution",
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
            "Segment is a pure coordinate point with no stored value",
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
            "Data movement consumes 60-80% of energy in modern systems",
            "energy-efficiency",
            "data-movement-cost",
        ),
        // nexus/index.llms.md
        claim(
            "n01",
            "nexus/index.llms.md",
            "FIH primitives are the only interface between any agents",
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
            "Core assembled from production-grade crates (petgraph, rusqlite, cyrs)",
            "architecture-approach",
            "crate-assembly",
        ),
        claim(
            "i02",
            "nexus/impl_init.llms.md",
            "Single codebase compiles to WASM (edge) and native (server)",
            "deployment-model",
            "unified-binary",
        ),
        claim(
            "i03",
            "nexus/impl_init.llms.md",
            "Cypher is the query language with richest LLM training data",
            "query-language",
            "cypher-first",
        ),
        claim(
            "i04",
            "nexus/impl_init.llms.md",
            "LLMs are accelerators, not requirements — Cairn proved 54/54 without LLMs",
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

// ── Phase 4: Add new documents (2 documents, 8 facts) ──────────────────

fn seed_new_documents(bb: &mut impl Blackboard) {
    let facts = [
        // nexus/notes/acp_nexus.llms.md
        claim(
            "a01",
            "nexus/notes/acp_nexus.llms.md",
            "ACP uses subprocess model with stdio transport for agent isolation",
            "agent-architecture",
            "subprocess-isolation",
        ),
        claim(
            "a02",
            "nexus/notes/acp_nexus.llms.md",
            "MCP/ACP/A2A form a three-layer protocol stack for agent ecosystems",
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
            "Multi-backend validation through ACP ensures cross-model comparison",
            "llm-backend",
            "provider-agnostic",
        ),
        // notes/iclr26_insight.llms.md
        claim(
            "c01",
            "notes/iclr26_insight.llms.md",
            "Mamba-3: inference-first with time-aware generalized trapezoidal discretization",
            "time-ontology",
            "time-aware-optimization",
        ),
        claim(
            "c02",
            "notes/iclr26_insight.llms.md",
            "Add arithmetic intensity check to MemoryLayout for compute-memory balance",
            "design-priority",
            "performance-first",
        ),
        claim(
            "c03",
            "notes/iclr26_insight.llms.md",
            "Single-layer diagonal SSMs cannot express non-Abelian state tracking",
            "expressivity-limits",
            "provable-ceiling",
        ),
        claim(
            "c04",
            "notes/iclr26_insight.llms.md",
            "Replace complexity matrix with roofline analysis for compiler feasibility",
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
    intents_submitted: usize,
    state: nexus_graph::BoardState,
}

fn do_tick(sched: &mut Scheduler<impl Blackboard + EvictCapable + Snapshottable>) -> TickResult {
    let intents_submitted = sched.tick().expect("tick");
    let state = Blackboard::read_state(&sched.bb);
    TickResult {
        intents_submitted,
        state,
    }
}

// ── Phase 5: simulate parallel hypothesis branches ─────────────────────

fn run_research_branch(
    sched: &mut Scheduler<impl Blackboard + EvictCapable + Snapshottable>,
    branch_name: &str,
    intent_filter: &str,
    conclusion: &str,
) {
    let state = Blackboard::read_state(&sched.bb);
    for intent in &state.intents {
        // Skip already-concluded, already-claimed, or stale intents
        if intent.to_fact_id.is_some() || intent.concluded_at.is_some() {
            continue;
        }
        if intent.worker.is_some() {
            continue;
        }
        if intent.description.contains(intent_filter) {
            let id = &intent.id.0;
            let c = serde_json::json!({
                "resolution": conclusion,
                "branch": branch_name,
            });
            sched.bb.claim_intent(id, branch_name).expect("claim");
            sched.bb.heartbeat(id, branch_name).expect("heartbeat");
            sched.bb.conclude_intent(id, &c).expect("conclude");
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════
//  Scenario tests
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_full_document_lifecycle() {
    // ── Phase 1: Seed initial corpus ───────────────────────────────
    let mut bb = create_blackboard();
    seed_initial_corpus(&mut bb);

    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));
    sched.register(Box::new(ContradictionDetector::new()));

    let state = Blackboard::read_state(&sched.bb);
    assert_eq!(state.facts.len(), 19, "Phase 1: 19 initial facts");

    // ── Phase 2: Gap detection ─────────────────────────────────────
    let r1 = do_tick(&mut sched);
    assert!(
        r1.intents_submitted > 0,
        "Phase 2: gap detector must find orphaned facts"
    );
    let gap_intents: Vec<_> = r1
        .state
        .intents
        .iter()
        .filter(|i| i.creator == "gap-detector")
        .collect();
    assert!(!gap_intents.is_empty(), "Gap detector produced intents");

    let gap_descs: Vec<&str> = gap_intents.iter().map(|i| i.description.as_str()).collect();
    let has_cross_origin = gap_descs.iter().any(|d| d.contains("Cross-origin gap"));
    assert!(
        has_cross_origin,
        "At least one cross-origin gap detected: {:?}",
        gap_descs
    );

    // ── Phase 3: Contradiction detection ────────────────────────────
    let r2 = do_tick(&mut sched);
    let contradiction_intents: Vec<_> = r2
        .state
        .intents
        .iter()
        .filter(|i| i.creator == "contradiction-detector")
        .collect();

    assert!(
        !contradiction_intents.is_empty(),
        "Phase 3: contradiction detector must find field-definition tension"
    );
    let contradiction_descs: Vec<&str> = contradiction_intents
        .iter()
        .map(|i| i.description.as_str())
        .collect();
    assert!(
        contradiction_descs
            .iter()
            .any(|d| d.contains("field-definition")),
        "field-definition contradiction found"
    );

    // ── Phase 4: New documents arrive ───────────────────────────────
    sched.register(Box::new(NewDocumentAnalyzer::with_baseline(initial_ids())));
    seed_new_documents(&mut sched.bb);

    let r3 = do_tick(&mut sched);
    let nda_intents: Vec<_> = r3
        .state
        .intents
        .iter()
        .filter(|i| i.creator == "new-document-analyzer")
        .collect();
    assert!(
        !nda_intents.is_empty(),
        "Phase 4: new-document analyzer must produce intents"
    );

    let plus_count = nda_intents
        .iter()
        .filter(|i| i.description.contains("+factor"))
        .count();
    let minus_count = nda_intents
        .iter()
        .filter(|i| i.description.contains("-factor"))
        .count();
    let gap_count = nda_intents
        .iter()
        .filter(|i| i.description.contains("Gap discovered"))
        .count();
    assert_eq!(
        plus_count + minus_count + gap_count,
        8,
        "All 8 new facts analyzed: +{} / -{} / gap {}",
        plus_count,
        minus_count,
        gap_count
    );
    assert!(
        minus_count > 0,
        "Must have at least one challenge (-factor)"
    );
    assert!(gap_count > 0, "Must have at least one new topic gap");

    // ── Phase 4b: Contradiction detector catches new tensions ───────
    let r4 = do_tick(&mut sched);
    let all_contradictions: Vec<_> = r4
        .state
        .intents
        .iter()
        .filter(|i| i.creator == "contradiction-detector")
        .collect();
    assert!(
        all_contradictions.len() >= 4,
        "Phase 4b: >=4 total contradictions: got {}",
        all_contradictions.len()
    );

    let descs_str: String = all_contradictions
        .iter()
        .map(|i| i.description.as_str())
        .collect::<Vec<_>>()
        .join(" | ");
    assert!(
        descs_str.contains("time-ontology"),
        "time-ontology tension: {}",
        descs_str
    );
    assert!(
        descs_str.contains("design-priority"),
        "design-priority tension: {}",
        descs_str
    );
    assert!(
        descs_str.contains("field-definition"),
        "field-definition tension: {}",
        descs_str
    );

    // ── Phase 5: Parallel research branches ────────────────────────
    run_research_branch(
        &mut sched,
        "branch-fidelity",
        "design-priority",
        "Structural fidelity is the primary constraint; performance is secondary",
    );
    run_research_branch(
        &mut sched,
        "branch-performance",
        "design-priority",
        "Roofline analysis must be a first-class compiler pass",
    );
    run_research_branch(
        &mut sched,
        "branch-time",
        "time-ontology",
        "Time as coordinate is ontological truth; discretization is a Field-layer optimization",
    );
    run_research_branch(
        &mut sched,
        "branch-field",
        "field-definition",
        "Abstract admissibility conditions and value-binding rules are complementary views",
    );

    let state = Blackboard::read_state(&sched.bb);
    let total_facts = state.facts.len();
    assert!(
        total_facts >= 23,
        "Phase 5: >=23 facts (19 initial + >=4 conclusions): got {}",
        total_facts
    );

    // ── Phase 6: Eviction + Snapshot ───────────────────────────────
    let state_before = Blackboard::read_state(&sched.bb);
    let intents_before = state_before.intents.len();
    EvictCapable::evict_before(&sched.bb, "9999999999").expect("evict");
    let state = Blackboard::read_state(&sched.bb);
    let remaining_intents = state.intents.len();
    assert!(
        remaining_intents < intents_before,
        "Phase 6a: concluded intents evicted: {} -> {}",
        intents_before,
        remaining_intents
    );

    // Snapshot round-trip
    let snapshot = Snapshottable::to_snapshot(&sched.bb);
    let json = serde_json::to_vec(&snapshot).expect("serialize");
    let restored: StorageSnapshot = serde_json::from_slice(&json).expect("deserialize");
    let mut bb_restored = create_blackboard_from_snapshot(restored);

    let state_restored = Blackboard::read_state(&bb_restored);
    assert_eq!(
        state_restored.facts.len(),
        total_facts,
        "Phase 6b: snapshot preserves all {} facts",
        total_facts
    );

    // Worker B continues from restored state
    bb_restored
        .submit_fact(&claim(
            "w01",
            "worker-b.nexus",
            "All four branches converge: SSCCS ontology is correct",
            "design-priority",
            "synthesis-resolution",
        ))
        .unwrap();

    let state_final = Blackboard::read_state(&bb_restored);
    assert_eq!(
        state_final.facts.len(),
        total_facts + 1,
        "Phase 6c: Worker B adds to restored blackboard"
    );
}

#[test]
fn scenario_duplicate_prevention_on_contradictions() {
    let mut bb = create_blackboard();
    seed_initial_corpus(&mut bb);
    seed_new_documents(&mut bb);

    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(ContradictionDetector::new()));

    let r1 = do_tick(&mut sched);
    let c1_count = r1
        .state
        .intents
        .iter()
        .filter(|i| i.creator == "contradiction-detector")
        .count();

    let r2 = do_tick(&mut sched);
    let c2_count = r2
        .state
        .intents
        .iter()
        .filter(|i| i.creator == "contradiction-detector")
        .count();

    assert_eq!(c1_count, c2_count, "No duplicate contradiction intents");
}

#[test]
fn scenario_new_document_no_duplicates() {
    let mut bb = create_blackboard();
    seed_initial_corpus(&mut bb);

    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(NewDocumentAnalyzer::with_baseline(initial_ids())));

    // First tick: no new facts (all in baseline)
    let r1 = do_tick(&mut sched);
    assert_eq!(r1.intents_submitted, 0, "First tick: 0 new intents");

    // Add new documents
    seed_new_documents(&mut sched.bb);
    let r2 = do_tick(&mut sched);
    let nda_count_2 = r2
        .state
        .intents
        .iter()
        .filter(|i| i.creator == "new-document-analyzer")
        .count();
    assert_eq!(nda_count_2, 8, "Second tick: 8 new facts analyzed");

    // Third tick: no new intents submitted (all facts already seen)
    let r3 = do_tick(&mut sched);
    assert_eq!(r3.intents_submitted, 0, "Third tick: no new intents");
}

// ── Cairn-style StateChangeDetector: ReasonCheckpoint pattern ─────────

#[test]
fn scenario_state_change_detector_cairn_pattern() {
    let bb = create_blackboard();

    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(StateChangeDetector::new()));

    // Tick 1: initialize silently (no checkpoint yet)
    let r1 = do_tick(&mut sched);
    assert_eq!(r1.intents_submitted, 0, "Tick 1: silent init");

    // Seed initial corpus
    seed_initial_corpus(&mut sched.bb);

    // Tick 2: fact count changed 0→19 → reason intent
    let r2 = do_tick(&mut sched);
    assert_eq!(
        r2.intents_submitted, 1,
        "Tick 2: reason triggered by facts:0->19"
    );
    let reason_intents: Vec<_> = r2
        .state
        .intents
        .iter()
        .filter(|i| i.creator == "state-change-detector")
        .collect();
    assert_eq!(reason_intents.len(), 1);
    assert!(reason_intents[0].description.contains("facts:0->19"));

    // Tick 3: no change → no intent
    let r3 = do_tick(&mut sched);
    assert_eq!(r3.intents_submitted, 0, "Tick 3: no change, no reason");

    // Add new documents
    seed_new_documents(&mut sched.bb);

    // Tick 4: facts 19→27 → reason intent
    let r4 = do_tick(&mut sched);
    assert_eq!(
        r4.intents_submitted, 1,
        "Tick 4: reason triggered by facts:19->27"
    );

    // Claim and conclude the reason intent (simulates agent work)
    let state = Blackboard::read_state(&sched.bb);
    let reason_id = &state
        .intents
        .iter()
        .find(|i| i.creator == "state-change-detector")
        .expect("reason intent exists")
        .id
        .0;
    sched.bb.claim_intent(reason_id, "analyst").expect("claim");
    sched.bb.heartbeat(reason_id, "analyst").expect("heartbeat");
    sched
        .bb
        .conclude_intent(
            reason_id,
            &serde_json::json!({"analysis": "8 new claims added from ACP and ICLR documents"}),
        )
        .expect("conclude");

    // Tick 5: open_intent count changed (reason concluded)
    // When the reason intent is concluded, open_intents drops, which
    // is a state change that triggers another reason (Cairn pattern).
    let r5 = do_tick(&mut sched);
    let reason_intents_t5: Vec<_> = r5
        .state
        .intents
        .iter()
        .filter(|i| i.creator == "state-change-detector")
        .collect();
    assert!(
        !reason_intents_t5.is_empty(),
        "Tick 5: reason triggered by open_intent completion (Cairn checkpoint pattern)"
    );

    // Snapshot round-trip: facts preserved
    let snapshot = Snapshottable::to_snapshot(&sched.bb);
    let json = serde_json::to_vec(&snapshot).expect("serialize");
    let restored: StorageSnapshot = serde_json::from_slice(&json).expect("deserialize");
    let bb_restored = create_blackboard_from_snapshot(restored);

    let state_restored = Blackboard::read_state(&bb_restored);
    assert_eq!(
        state_restored.facts.len(),
        28,
        "Snapshot preserves 28 facts (19 initial + 8 new + 1 conclusion)"
    );
}
