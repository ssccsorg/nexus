// Full-flow reference test: validates the entire Nexus pipeline end-to-end.
//
// Tests:
//   1. Seed facts → scheduler tick → gap detector creates Intents
//   2. Claim → heartbeat → conclude → new Fact
//   3. Multiple ticks converge (no duplicate Intents)
//   4. Eviction removes stale concluded intents
//
// This is the canonical "many iterations" validation:
//   simple rules + repetition → correct emergent behavior.

use nexus_graph::{Blackboard, EvictCapable, Fact, FihHash, Intent, create_blackboard};
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

    let _ = EvictCapable::evict_before(&sched.bb, "9999999999").expect("evict");
    let state = Blackboard::read_state(&sched.bb);
    assert_eq!(state.intents.len(), 0, "all concluded intents evicted");
}

#[test]
fn flow_no_duplicates() {
    let mut bb = create_blackboard();
    seed_corpus(&mut bb);

    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));

    for tick in 0..10 {
        let n = sched.tick().expect("tick");
        if tick == 0 {
            assert_eq!(n, 2, "first tick creates intents");
        } else {
            assert_eq!(n, 0, "tick {tick}: no duplicates");
        }
    }

    let state = Blackboard::read_state(&sched.bb);
    assert_eq!(state.intents.len(), 2);
}
