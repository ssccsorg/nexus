// Concurrency tests against FihStorage<SimFihIo>.
//
// FihStorage uses RwLock internally (intent_cache, fact_cache, etc.),
// so the outer Mutex wrapper used in the original nex tests is unnecessary.
// Direct Arc<FihStorage<SimFihIo>> ensures all internal RwLock
// contention is tested at the storage level.

mod common;

use std::sync::{Arc, Barrier};
use std::thread;

use nexus_model::{BlackboardError, FactCapable, IntentCapable, StorageRead};
use nexus_storage_sim::{FihStorage, SimFihIo};

type SharedStorage = Arc<FihStorage<SimFihIo>>;

fn bb() -> SharedStorage {
    Arc::new(FihStorage::new(SimFihIo::new(), "test"))
}

fn setup_intent(bb: &SharedStorage, iid: &str) {
    bb.submit_fact(&common::fact("f_base")).unwrap();
    bb.submit_intent(&common::intent(iid, vec!["f_base"]))
        .unwrap();
}

// ── Test 1: concurrent claim of the same intent → Conflict ─────────────

#[test]
fn test_concurrent_claim_same_intent() {
    let bb = bb();
    setup_intent(&bb, "i_conflict");

    let bb1 = Arc::clone(&bb);
    let bb2 = Arc::clone(&bb);
    let barrier = Arc::new(Barrier::new(2));
    let b1 = Arc::clone(&barrier);
    let b2 = Arc::clone(&barrier);

    let h1 = thread::spawn(move || {
        b1.wait();
        bb1.claim_intent("i_conflict", "agent-a")
    });

    let h2 = thread::spawn(move || {
        b2.wait();
        bb2.claim_intent("i_conflict", "agent-b")
    });

    let r1 = h1.join().unwrap();
    let r2 = h2.join().unwrap();

    match (&r1, &r2) {
        (Ok(()), Ok(())) => panic!("both claimed same intent"),
        (Err(BlackboardError::Conflict(_)), Err(BlackboardError::Conflict(_))) => {
            panic!("both got conflict - one should succeed")
        }
        (Ok(()), Err(BlackboardError::Conflict(_)))
        | (Err(BlackboardError::Conflict(_)), Ok(())) => {}
        _ => panic!("unexpected: {:?}, {:?}", r1, r2),
    }
}

// ── Test 2: concurrent claim of different intents → both succeed ───────

#[test]
fn test_concurrent_claim_different_intents() {
    let bb = bb();
    bb.submit_fact(&common::fact("f_base")).unwrap();
    bb.submit_intent(&common::intent("i_a", vec!["f_base"]))
        .unwrap();
    bb.submit_intent(&common::intent("i_b", vec!["f_base"]))
        .unwrap();

    let bb1 = Arc::clone(&bb);
    let bb2 = Arc::clone(&bb);

    let h1 = thread::spawn(move || bb1.claim_intent("i_a", "agent-a"));
    let h2 = thread::spawn(move || bb2.claim_intent("i_b", "agent-b"));

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
                bb.submit_fact(&common::fact(&format!("f_t{}_i{}", t, i)))
                    .unwrap();
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let state = bb.read_state();
    assert_eq!(state.facts.len(), 1000, "all 10x100 facts submitted");
}

// ── Test 4: concurrent read during write — no panic ─────────────────────

#[test]
fn test_concurrent_read_during_write() {
    let bb = bb();

    for i in 0..50 {
        bb.submit_fact(&common::fact(&format!("f_init_{}", i)))
            .unwrap();
    }

    let bb_write = Arc::clone(&bb);
    let writer = thread::spawn(move || {
        for i in 0..200 {
            bb_write
                .submit_fact(&common::fact(&format!("f_write_{}", i)))
                .unwrap();
        }
    });

    let bb_read = Arc::clone(&bb);
    let reader = thread::spawn(move || {
        for _ in 0..50 {
            let state = bb_read.read_state();
            assert!(state.facts.len() >= 50);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    });

    writer.join().unwrap();
    reader.join().unwrap();
}

// ── Test 5: release then re-claim by different agent succeeds ──────────

#[test]
fn test_release_then_reclaim() {
    let bb = bb();
    setup_intent(&bb, "i_rel");

    bb.claim_intent("i_rel", "agent-a").unwrap();
    bb.release_intent("i_rel", "agent-a").unwrap();
    bb.claim_intent("i_rel", "agent-b").unwrap();
    let result = bb.heartbeat("i_rel", "agent-b");
    assert!(result.is_ok(), "heartbeat after re-claim succeeds");
}

// ── Test 6: release then re-claim and conclude succeeds ────────────────

#[test]
fn test_release_then_reclaim_conclude() {
    let bb = bb();
    setup_intent(&bb, "i_car");

    bb.claim_intent("i_car", "agent-a").unwrap();
    bb.release_intent("i_car", "agent-a").unwrap();
    bb.claim_intent("i_car", "agent-b").unwrap();
    let result = bb.conclude_intent("i_car", "done");
    assert!(result.is_ok(), "conclude after re-claim succeeds");
}
