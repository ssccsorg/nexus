// Full-flow reference tests: validates the entire Nexus pipeline end-to-end.
//
// Refactored for correct FIH semantics: detectors produce Facts (observations),
// agents read Facts and create Intents (actions).
//
// Scenarios:
//   1. OODA with gap detector — seed → tick → verify gap Facts created
//   2. Agent reads gap Fact → creates Intent → claim → conclude
//   3. Eviction — conclude → evict_before → verify removal
//   4. Duplicate prevention — gap Facts stable across ticks
//   5. Cross-worker snapshot — Worker A builds state → Worker B restores

use nexus_graph::{
    Blackboard, EvictCapable, Fact, FihHash, Intent, Snapshottable, StorageSnapshot,
    create_blackboard, create_blackboard_from_snapshot,
};
use nexus_process::scheduler::Scheduler;
use nexus_process::tasks::gap_detector::GapDetector;

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

/// Count detector Facts by creator.
fn count_detector_facts(state: &nexus_graph::BoardState, creator: &str) -> usize {
    state.facts.iter().filter(|f| f.creator == creator).count()
}

#[test]
fn flow_ooda_with_gap_detector() {
    let mut bb = create_blackboard();
    seed_corpus(&mut bb);

    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));

    // Gap detector produces Facts, not Intents
    let facts_submitted = sched.tick().expect("first tick");
    assert!(facts_submitted > 0, "gap detector produces gap facts");
    let facts_submitted_2 = sched.tick().expect("second tick");
    assert_eq!(facts_submitted_2, 0, "no new gap facts on second tick");
    let state = Blackboard::read_state(&sched.bb);
    assert!(
        state.facts.len() > 5,
        "original facts + gap facts: {}",
        state.facts.len()
    );
    assert!(
        count_detector_facts(&state, "gap-detector") > 0,
        "gap facts exist"
    );
    // No intents (detectors don't create intents anymore)
    assert_eq!(state.intents.len(), 0);
}

#[test]
fn flow_agent_creates_intent_from_detector_fact() {
    let mut bb = create_blackboard();
    seed_corpus(&mut bb);
    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));
    sched.tick().expect("tick");

    // Agent reads gap Facts from detector, creates an Intent
    let state = Blackboard::read_state(&sched.bb);
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
        concluded_at: None,
    };
    let iid = sched.bb.submit_intent(&intent).expect("submit");
    sched.bb.claim_intent(&iid.0, "agent-alpha").expect("claim");
    sched
        .bb
        .heartbeat(&iid.0, "agent-alpha")
        .expect("heartbeat");

    let result = serde_json::json!("synthesis complete");
    let new_fact = sched.bb.conclude_intent(&iid.0, &result).expect("conclude");
    assert_eq!(new_fact.content, result);

    let state = Blackboard::read_state(&sched.bb);
    assert!(
        state.facts.iter().any(|f| f.content == result),
        "conclusion fact exists"
    );
    assert!(
        state.facts.len() > 6,
        "facts: original + gap + conclusion = {}",
        state.facts.len()
    );
}

#[test]
fn flow_eviction() {
    let mut bb = create_blackboard();
    seed_corpus(&mut bb);
    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));
    sched.tick().expect("tick");

    // Agent creates and concludes an intent
    let intent = Intent {
        id: FihHash("evict-test".into()),
        from_facts: vec!["f_001".into()],
        description: "test".into(),
        creator: "evictor".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        concluded_at: None,
    };
    let iid = sched.bb.submit_intent(&intent).expect("submit");
    sched.bb.claim_intent(&iid.0, "evictor").expect("claim");
    sched
        .bb
        .conclude_intent(&iid.0, &serde_json::json!("done"))
        .expect("conclude");

    EvictCapable::evict_before(&sched.bb, "9999999999").expect("evict");
    let state = Blackboard::read_state(&sched.bb);
    // evict_before removes orphaned facts not referenced by kept intents.
    assert_eq!(state.intents.len(), 0, "concluded intent evicted");
    assert!(
        state.facts.len() >= 2,
        "referenced facts persist: {}",
        state.facts.len()
    );
}

#[test]
fn flow_no_duplicates() {
    let mut bb = create_blackboard();
    seed_corpus(&mut bb);
    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));

    // First tick produces gap facts
    let n0 = sched.tick().expect("tick 0");
    assert!(n0 > 0, "tick 0: gap facts produced");
    let state0 = Blackboard::read_state(&sched.bb);
    let gap_count_0 = count_detector_facts(&state0, "gap-detector");

    // Subsequent ticks: no new gap facts (same data, already observed)
    for tick in 1..10 {
        let n = sched.tick().expect("tick");
        assert_eq!(n, 0, "tick {tick}: no new facts");
    }
    let state = Blackboard::read_state(&sched.bb);
    let gap_count_final = count_detector_facts(&state, "gap-detector");
    assert_eq!(
        gap_count_0, gap_count_final,
        "gap facts stable across ticks"
    );
}

// ── Cross-worker snapshot scenario ─────────────────────────────────────

#[test]
fn flow_cross_worker_snapshot() {
    let mut bb_a = create_blackboard();
    seed_corpus(&mut bb_a);
    let mut sched = Scheduler::new(bb_a);
    sched.register(Box::new(GapDetector::new()));
    sched.tick().expect("worker A tick");

    let state_a = Blackboard::read_state(&sched.bb);
    let facts_a = state_a.facts.len();
    let gap_a = count_detector_facts(&state_a, "gap-detector");

    // Worker A: snapshot → JSON
    let snapshot_a = Snapshottable::to_snapshot(&sched.bb);
    let json = serde_json::to_vec(&snapshot_a).expect("serialize");

    // Worker B: restore from snapshot
    let snapshot_b: StorageSnapshot = serde_json::from_slice(&json).expect("deserialize");
    let mut bb_b = create_blackboard_from_snapshot(snapshot_b);

    let state = Blackboard::read_state(&bb_b);
    assert_eq!(
        state.facts.len(),
        facts_a,
        "Worker B sees same facts as Worker A"
    );
    let gap_b = count_detector_facts(&state, "gap-detector");
    assert_eq!(gap_a, gap_b, "gap facts preserved in snapshot");

    // Worker B: adds new fact, creates and concludes intent
    bb_b.submit_fact(&Fact {
        id: FihHash("f_worker_b_001".into()),
        origin: "worker-b".into(),
        content: serde_json::json!("Worker B discovery"),
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
        concluded_at: None,
    };
    let iid = bb_b.submit_intent(&intent).expect("submit");
    bb_b.claim_intent(&iid.0, "worker-b").expect("claim");
    bb_b.conclude_intent(&iid.0, &serde_json::json!("confirmed by Worker B"))
        .expect("conclude");

    let state = Blackboard::read_state(&bb_b);
    assert_eq!(
        state.facts.len(),
        facts_a + 2,
        "A's facts + 1 new + 1 conclusion"
    );
    assert_eq!(
        state.intents.len(),
        1,
        "Worker B's intent persists (not yet evicted)"
    );
}
