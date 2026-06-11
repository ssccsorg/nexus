// Nexus Process Integration Tests
// ================================
// Executable specification of the core FIH (Fact/Intent/Hint) lifecycle.
// These tests define the contract between the process layer and the
// storage/graph layers. Every test documents the expected behavior
// that any Blackboard implementation must satisfy.
//
// FIH Lifecycle Contract (the "Cairn pattern"):
//
//   submit_fact(claim)         ← ingester: raw knowledge enters the system
//   detector.orient(state)     ← scheduler: every tick, detectors observe
//   detector → Fact(gap)      ← detector: records pattern as immutable Fact
//   agent reads Fact(gap)     ← agent: perceives detector observation
//   agent → submit_intent()   ← agent: proposes action based on observation
//   agent → claim_intent()    ← agent: takes ownership of the exploration
//   agent → heartbeat()       ← agent: signals liveness during work
//   agent → conclude_intent() ← agent: produces result → new Fact created
//   evict_before(timestamp)   ← scheduler: removes concluded intents only
//
// Key architectural constraint: detectors NEVER create Intents.
// Intents are agent actions. Facts are observations.
// This separation is the core FIH semantics enforcement.

use nex::process::scheduler::Scheduler;
use nex::process::tasks::gap_detector::GapDetector;
use nex::storage::petgraph::{Snapshottable, StorageSnapshot};
use nex::{
    Blackboard, BoardState, Content, DefaultBlackboard, EvictCapable, Fact, FactCapable, FihHash,
    Intent, IntentCapable, StorageRead, create_blackboard,
};

fn seed_corpus(bb: &mut impl Blackboard) {
    let corpus = [
        ("f_001", "arxiv", "GNN achieves 92% accuracy"),
        ("f_002", "arxiv", "Oversmoothing beyond 6 layers"),
        ("f_003", "nature", "Deep learning needs 10x more data"),
        ("f_004", "nature", "Transformers outperform RNNs"),
        ("f_005", "iclr", "Attention is all you need"),
    ];
    for (id, origin, content) in &corpus {
        bb.submit_fact(&Fact {
            id: FihHash(id.to_string()),
            origin: origin.to_string(),
            content: (*content).into(),
            creator: "corpus".into(),
        })
        .unwrap();
    }
}

fn count_detector_facts(state: &BoardState, creator: &str) -> usize {
    state.facts.iter().filter(|f| f.creator == creator).count()
}

// ─────────────────────────────────────────────────────────────────────────
// Test: flow_ooda_with_gap_detector
//
// Verifies: Scheduler → GapDetector → Facts flow.
//
// Sequence:
//   1. Seed 5 facts from 3 origins (arxiv×2, nature×2, iclr×1)
//   2. Scheduler.tick() calls GapDetector.orient(state)
//   3. GapDetector finds orphaned facts per origin, records gap Facts
//   4. Second tick: same data → no new gap Facts (duplicate prevention)
//
// Core layer exercised: Scheduler, DetectionCapable trait, GapDetector,
//   Blackboard::submit_fact, StorageRead::read_state
// ─────────────────────────────────────────────────────────────────────────
#[test]
fn flow_ooda_with_gap_detector() {
    let mut bb = create_blackboard();
    seed_corpus(&mut bb);

    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));

    let facts_submitted = sched.tick().expect("first tick");
    assert!(facts_submitted > 0, "gap detector produces gap facts");
    let facts_submitted_2 = sched.tick().expect("second tick");
    assert_eq!(facts_submitted_2, 0, "no new gap facts on second tick");
    let state = StorageRead::read_state(&sched.bb);
    assert!(
        state.facts.len() > 5,
        "original facts + gap facts: {}",
        state.facts.len()
    );
    assert!(
        count_detector_facts(&state, "gap-detector") > 0,
        "gap facts exist"
    );
    assert_eq!(state.intents.len(), 0);
}

// ─────────────────────────────────────────────────────────────────────────
// Test: flow_agent_creates_intent_from_detector_fact
//
// Verifies: Agent reads detector Fact → creates Intent → conclude cycle.
//
// This is the CORRECT FIH flow (after detector refactoring):
//   1. Detector produces gap Facts (observations)
//   2. Agent reads gap Facts from read_state()
//   3. Agent creates Intent referencing those Facts
//   4. Agent claims, heartbeats, concludes the Intent
//   5. Conclusion creates a new Fact
//
// Core layer exercised: Blackboard::submit_intent, claim_intent,
//   heartbeat, conclude_intent, read_state
// Architectural guarantee: detector output (Facts) ≠ agent output (Intents)
// ─────────────────────────────────────────────────────────────────────────
#[test]
fn flow_agent_creates_intent_from_detector_fact() {
    let mut bb = create_blackboard();
    seed_corpus(&mut bb);
    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));
    sched.tick().expect("tick");

    let state = StorageRead::read_state(&sched.bb);
    let gap_facts: Vec<&Fact> = state
        .facts
        .iter()
        .filter(|f| f.creator == "gap-detector")
        .collect();
    assert!(!gap_facts.is_empty(), "detector produced gap facts");

    let intent = Intent {
        id: FihHash("agent-intent-1".into()),
        from_facts: gap_facts.iter().map(|f| f.id.0.clone()).collect(),
        description: "Investigate gap".into(),
        creator: "agent-alpha".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        concluded: false,
        concluded_at: None,
    };
    let iid = sched.bb.submit_intent(&intent).expect("submit");
    sched.bb.claim_intent(&iid.0, "agent-alpha").expect("claim");
    sched
        .bb
        .heartbeat(&iid.0, "agent-alpha")
        .expect("heartbeat");

    let new_fact = sched
        .bb
        .conclude_intent(&iid.0, "synthesis complete")
        .expect("conclude");
    assert_eq!(new_fact.content, Content::from("synthesis complete"));

    let state = StorageRead::read_state(&sched.bb);
    assert!(
        state
            .facts
            .iter()
            .any(|f| f.content == "synthesis complete"),
        "conclusion fact exists"
    );
    assert!(
        state.facts.len() > 6,
        "facts: original + gap + conclusion = {}",
        state.facts.len()
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Test: flow_eviction
//
// Verifies: evict_before removes concluded intents.
//
// Sequence:
//   1. Agent creates + concludes an Intent
//   2. evict_before("9999999999") — cutoff far in the future
//   3. Concluded intent is removed from state.intents
//   4. Referenced facts survive (they're still needed)
//
// Core layer exercised: EvictCapable::evict_before, StorageRead::read_state
// Stigmergy metaphor: pheromone evaporation — old signals decay
// ─────────────────────────────────────────────────────────────────────────
#[test]
fn flow_eviction() {
    let mut bb = create_blackboard();
    seed_corpus(&mut bb);
    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));
    sched.tick().expect("tick");

    let intent = Intent {
        id: FihHash("evict-test".into()),
        from_facts: vec!["f_001".into()],
        description: "test".into(),
        creator: "evictor".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        concluded: false,
        concluded_at: None,
    };
    let iid = sched.bb.submit_intent(&intent).expect("submit");
    sched.bb.claim_intent(&iid.0, "evictor").expect("claim");
    sched.bb.conclude_intent(&iid.0, "done").expect("conclude");

    EvictCapable::evict_before(&sched.bb, "9999999999").expect("evict");
    let state = StorageRead::read_state(&sched.bb);
    assert_eq!(state.intents.len(), 0, "concluded intent evicted");
    assert!(
        state.facts.len() >= 2,
        "referenced facts persist: {}",
        state.facts.len()
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Test: flow_no_duplicates
//
// Verifies: detectors are idempotent across ticks.
//
// Sequence:
//   1. Tick 0: detector produces gap Facts for current orphan set
//   2. Ticks 1-9: same data → no new Facts (seen set prevents duplicates)
//   3. Gap Facts count stable across all ticks
//
// Core layer exercised: GapDetector.seen_origin, DetectionCapable::orient
// Stigmergy guarantee: detectors observe, don't re-observe same pattern
// ─────────────────────────────────────────────────────────────────────────
#[test]
fn flow_no_duplicates() {
    let mut bb = create_blackboard();
    seed_corpus(&mut bb);
    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));

    let n0 = sched.tick().expect("tick 0");
    assert!(n0 > 0, "tick 0: gap facts produced");
    let state0 = StorageRead::read_state(&sched.bb);
    let gap_count_0 = count_detector_facts(&state0, "gap-detector");

    for tick in 1..10 {
        let n = sched.tick().expect("tick");
        assert_eq!(n, 0, "tick {tick}: no new facts");
    }
    let state = StorageRead::read_state(&sched.bb);
    let gap_count_final = count_detector_facts(&state, "gap-detector");
    assert_eq!(
        gap_count_0, gap_count_final,
        "gap facts stable across ticks"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Test: flow_cross_worker_snapshot
//
// Verifies: knowledge persists across worker boundaries via snapshot.
//
// This is the stigmergy persistence pattern:
//   1. Worker A: seeds corpus, runs detector, produces analysis Facts
//   2. Worker A: to_snapshot() → JSON (simulates R2 blob storage)
//   3. Worker B: from_snapshot(JSON) → restores full state
//   4. Worker B: sees Worker A's facts, continues work independently
//   5. Worker B: submits new fact, creates & concludes intent
//
// Core layer exercised: Snapshottable::to_snapshot,
//   create_blackboard_from_snapshot, StorageSnapshot serialization
// Stigmergy guarantee: workers communicate only through shared state,
//   never through direct calls
// ─────────────────────────────────────────────────────────────────────────
#[test]
fn flow_cross_worker_snapshot() {
    let mut bb_a = create_blackboard();
    seed_corpus(&mut bb_a);
    let mut sched = Scheduler::new(bb_a);
    sched.register(Box::new(GapDetector::new()));
    sched.tick().expect("worker A tick");

    let state_a = StorageRead::read_state(&sched.bb);
    let facts_a = state_a.facts.len();
    let gap_a = count_detector_facts(&state_a, "gap-detector");

    let snapshot_a = Snapshottable::to_snapshot(&sched.bb);
    let json = serde_json::to_vec(&snapshot_a).expect("serialize");

    let snapshot_b: StorageSnapshot = serde_json::from_slice(&json).expect("deserialize");
    let bb_b = <DefaultBlackboard as Snapshottable>::from_snapshot(snapshot_b);

    let state = StorageRead::read_state(&bb_b);
    assert_eq!(
        state.facts.len(),
        facts_a,
        "Worker B sees same facts as Worker A"
    );
    let gap_b = count_detector_facts(&state, "gap-detector");
    assert_eq!(gap_a, gap_b, "gap facts preserved in snapshot");

    bb_b.submit_fact(&Fact {
        id: FihHash("f_worker_b_001".into()),
        origin: "worker-b".into(),
        content: Content {
            mime_type: "application/json".into(),
            data: serde_json::json!("Worker B discovery")
                .to_string()
                .into_bytes(),
        },
        creator: "worker-b".into(),
    })
    .unwrap();

    let intent = Intent {
        id: FihHash("wb-intent".into()),
        from_facts: vec!["f_worker_b_001".into()],
        description: "Explore Worker B discovery".into(),
        creator: "worker-b".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        concluded: false,
        concluded_at: None,
    };
    let iid = bb_b.submit_intent(&intent).expect("submit");
    bb_b.claim_intent(&iid.0, "worker-b").expect("claim");
    bb_b.conclude_intent(&iid.0, "confirmed by Worker B")
        .expect("conclude");

    let state = StorageRead::read_state(&bb_b);
    assert_eq!(
        state.facts.len(),
        facts_a + 2,
        "A's facts + 1 new + 1 conclusion"
    );
    assert_eq!(state.intents.len(), 1, "Worker B's intent persists");
}
