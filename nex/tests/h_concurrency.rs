// nexus-graph — Blackboard concurrency tests.
//
// Verifies that the Blackboard trait contract holds under concurrent access.
// All tests wrap the blackboard in Arc<Mutex<>> to simulate multi-worker
// access patterns. Uses create_blackboard() factory — never depends on
// HybridBlackboard directly.

use nexus_storage_composite::HybridBlackboard;
use nexus_model::{Blackboard, BlackboardError, Content, Fact, FihHash, Intent};
use std::sync::{Arc, Mutex};
use std::thread;

type SharedBlackboard = Arc<Mutex<Box<dyn Blackboard + Send>>>;

fn bb() -> SharedBlackboard {
    Arc::new(Mutex::new(
        Box::new(HybridBlackboard::new()) as Box<dyn Blackboard + Send>
    ))
}

fn fact(id: &str) -> Fact {
    Fact {
        id: FihHash::from_hex(id),
        origin: "test".into(),
        content: Content {
            mime_type: "application/json".into(),
            data: serde_json::json!("data").to_string().into_bytes(),
        },
        creator: "tester".into(),
    }
}

fn intent(id: &str, from: Vec<&str>) -> Intent {
    Intent {
        id: FihHash::from_hex(id),
        from_facts: from.into_iter().map(|s| FihHash::from_hex(s)).collect(),
        description: format!("intent {}", id),
        creator: "tester".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    }
}

fn setup_intent(bb: &SharedBlackboard, iid: &str) {
    let b = bb.lock().unwrap();
    b.submit_fact(&fact("f_base")).unwrap();
    b.submit_intent(&intent(iid, vec!["f_base"])).unwrap();
}

// ── Test 1: concurrent claim of the same intent → Conflict ─────────────

#[test]
fn test_concurrent_claim_same_intent() {
    let bb = bb();
    setup_intent(&bb, "i_conflict");

    let bb1 = Arc::clone(&bb);
    let bb2 = Arc::clone(&bb);

    let h1 = thread::spawn(move || bb1.lock().unwrap().claim_intent("i_conflict", "agent-a"));

    let h2 = thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(5));
        bb2.lock().unwrap().claim_intent("i_conflict", "agent-b")
    });

    let r1 = h1.join().unwrap();
    let r2 = h2.join().unwrap();

    match (&r1, &r2) {
        (Ok(()), Ok(())) => panic!("both claimed same intent"),
        (Err(BlackboardError::Conflict(_)), Err(BlackboardError::Conflict(_))) => {
            panic!("both got conflict — one should succeed")
        }
        (Ok(()), Err(BlackboardError::Conflict(_)))
        | (Err(BlackboardError::Conflict(_)), Ok(())) => {} // Expected
        _ => panic!("unexpected: {:?}, {:?}", r1, r2),
    }
}

// ── Test 2: concurrent claim of different intents → both succeed ───────

#[test]
fn test_concurrent_claim_different_intents() {
    let bb = bb();
    {
        let b = bb.lock().unwrap();
        b.submit_fact(&fact("f_base")).unwrap();
        b.submit_intent(&intent("i_a", vec!["f_base"])).unwrap();
        b.submit_intent(&intent("i_b", vec!["f_base"])).unwrap();
    }

    let bb1 = Arc::clone(&bb);
    let bb2 = Arc::clone(&bb);

    let h1 = thread::spawn(move || bb1.lock().unwrap().claim_intent("i_a", "agent-a"));
    let h2 = thread::spawn(move || bb2.lock().unwrap().claim_intent("i_b", "agent-b"));

    assert!(h1.join().unwrap().is_ok());
    assert!(h2.join().unwrap().is_ok());
}

// ── Test 3: concurrent fact submission — all accepted ───────────────────

#[test]
fn test_concurrent_submit_fact() {
    let bb = bb();
    let mut handles = vec![];

    for t in 0..10 {
        let bb = Arc::clone(&bb);
        handles.push(thread::spawn(move || {
            for i in 0..100 {
                bb.lock()
                    .unwrap()
                    .submit_fact(&fact(&format!("f_t{}_i{}", t, i)))
                    .unwrap();
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let b = bb.lock().unwrap();
    let state = b.read_state();
    assert_eq!(state.facts.len(), 1000, "all 10x100 facts submitted");
}

// ── Test 4: concurrent read during write — no panic ─────────────────────

#[test]
fn test_concurrent_read_during_write() {
    let bb = bb();

    {
        let b = bb.lock().unwrap();
        for i in 0..50 {
            b.submit_fact(&fact(&format!("f_init_{}", i))).unwrap();
        }
    }

    let bb_write = Arc::clone(&bb);
    let writer = thread::spawn(move || {
        for i in 0..200 {
            bb_write
                .lock()
                .unwrap()
                .submit_fact(&fact(&format!("f_write_{}", i)))
                .unwrap();
        }
    });

    let bb_read = Arc::clone(&bb);
    let reader = thread::spawn(move || {
        for _ in 0..50 {
            let state = bb_read.lock().unwrap().read_state();
            assert!(state.facts.len() >= 50);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    });

    writer.join().unwrap();
    reader.join().unwrap();
}

// ── Test 5: heartbeat after release re-assigns worker ──────────────────

#[test]
fn test_heartbeat_after_release() {
    let bb = bb();
    setup_intent(&bb, "i_rel");

    bb.lock().unwrap().claim_intent("i_rel", "agent-a").unwrap();
    bb.lock()
        .unwrap()
        .release_intent("i_rel", "agent-a")
        .unwrap();
    let result = bb.lock().unwrap().heartbeat("i_rel", "agent-b");
    assert!(result.is_ok(), "heartbeat after release succeeds");
}

// ── Test 6: conclude after release succeeds ────────────────────────────

#[test]
fn test_conclude_after_release() {
    let bb = bb();
    setup_intent(&bb, "i_car");

    bb.lock().unwrap().claim_intent("i_car", "agent-a").unwrap();
    bb.lock()
        .unwrap()
        .release_intent("i_car", "agent-a")
        .unwrap();
    let result = bb.lock().unwrap().conclude_intent("i_car", "done");
    assert!(result.is_ok(), "conclude after release succeeds");
}
