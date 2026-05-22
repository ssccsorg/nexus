// Full-flow reference tests: validates the entire Nexus pipeline end-to-end.
//
// Scenarios:
//   1. OODA with gap detector — seed → tick → verify Intents created
//   2. Intent lifecycle — claim → heartbeat → conclude → new Fact
//   3. Eviction — conclude → evict_before → verify removal
//   4. Duplicate prevention — 10 ticks, only first creates Intents
//   5. Cross-worker snapshot — Worker A builds state → Worker B restores → continues

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

#[test]
fn flow_ooda_with_gap_detector() {
    let mut bb = create_blackboard();
    seed_corpus(&mut bb);

    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));

    let submitted = sched.tick().expect("first tick");
    assert_eq!(submitted, 2, "2 origins with >=2 orphans");
    let submitted = sched.tick().expect("second tick");
    assert_eq!(submitted, 0, "no new orphans");
    let state = Blackboard::read_state(&sched.bb);
    assert_eq!(state.facts.len(), 5);
    assert_eq!(state.intents.len(), 2);
}

#[test]
fn flow_intent_lifecycle() {
    let mut bb = create_blackboard();
    seed_corpus(&mut bb);
    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));
    sched.tick().expect("tick");

    let state = Blackboard::read_state(&sched.bb);
    let intent_id = state.intents[0].id.0.clone();
    sched
        .bb
        .claim_intent(&intent_id, "agent-alpha")
        .expect("claim");
    sched
        .bb
        .heartbeat(&intent_id, "agent-alpha")
        .expect("heartbeat");

    let result = serde_json::json!("synthesis complete");
    let new_fact = sched
        .bb
        .conclude_intent(&intent_id, &result)
        .expect("conclude");
    assert_eq!(new_fact.content, result);
    let state = Blackboard::read_state(&sched.bb);
    assert_eq!(state.facts.len(), 6);
}

#[test]
fn flow_eviction() {
    let mut bb = create_blackboard();
    seed_corpus(&mut bb);
    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));
    sched.tick().expect("tick");

    let state = Blackboard::read_state(&sched.bb);
    let ids: Vec<String> = state.intents.iter().map(|i| i.id.0.clone()).collect();
    for id in &ids {
        sched.bb.claim_intent(id, "evictor").expect("claim");
        sched
            .bb
            .conclude_intent(id, &serde_json::json!("done"))
            .expect("conclude");
    }
    EvictCapable::evict_before(&sched.bb, "9999999999").expect("evict");
    let state = Blackboard::read_state(&sched.bb);
    assert_eq!(state.intents.len(), 0);
}

#[test]
fn flow_no_duplicates() {
    let mut bb = create_blackboard();
    seed_corpus(&mut bb);
    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));
    for tick in 0..10 {
        let n = sched.tick().expect("tick");
        assert_eq!(n, if tick == 0 { 2 } else { 0 }, "tick {tick}");
    }
    let state = Blackboard::read_state(&sched.bb);
    assert_eq!(state.intents.len(), 2);
}

// ── Cross-worker snapshot scenario: stigmergy via serialised state ─────────
//
// Worker A: seeds facts, runs scheduler, creates Intents, snapshots state
// Worker B: restores from snapshot, continues work, submits new conclusions
// No direct communication — only through the snapshot.

#[test]
fn flow_cross_worker_snapshot() {
    let mut bb_a = create_blackboard();
    seed_corpus(&mut bb_a);
    let mut sched = Scheduler::new(bb_a);
    sched.register(Box::new(GapDetector::new()));
    sched.tick().expect("worker A tick");

    // Worker A: snapshot → JSON (simulates R2 storage)
    let snapshot_a = Snapshottable::to_snapshot(&sched.bb);
    let json = serde_json::to_vec(&snapshot_a).expect("serialize");

    // Worker B: restore from snapshot
    let snapshot_b: StorageSnapshot = serde_json::from_slice(&json).expect("deserialize");
    let mut bb_b = create_blackboard_from_snapshot(snapshot_b);

    // Worker B: verify it sees Worker A's data
    let state = Blackboard::read_state(&bb_b);
    assert_eq!(state.facts.len(), 5);
    assert_eq!(state.intents.len(), 2);

    // Worker B: submit new facts, claim and conclude Worker A's Intent
    bb_b.submit_fact(&Fact {
        id: FihHash("f_worker_b_001".into()),
        origin: "worker-b".into(),
        content: serde_json::json!("Worker B discovery"),
        creator: "worker-b".into(),
    })
    .unwrap();

    let state = Blackboard::read_state(&bb_b);
    let intent_id = state.intents[0].id.0.clone();
    bb_b.claim_intent(&intent_id, "worker-b").expect("claim");
    bb_b.conclude_intent(&intent_id, &serde_json::json!("confirmed by Worker B"))
        .expect("conclude");

    let state = Blackboard::read_state(&bb_b);
    assert_eq!(state.facts.len(), 7, "5 original + 1 new + 1 conclusion");
    assert_eq!(state.intents.len(), 2);
}
