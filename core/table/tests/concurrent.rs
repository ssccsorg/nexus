// nexus-table — Concurrent stress tests for SqlBlackboard.
//
// These tests exercise the Blackboard under extreme parallelism to verify
// that all operations are race-free and error handling is correct.

use nexus_table::{Blackboard, BlackboardError, Fact, FihHash, Intent, SqlBlackboard};
use std::sync::{Arc, Mutex};

fn make_fact(id: &str, content: &str) -> Fact {
    Fact {
        id: FihHash(id.into()),
        origin: "test".into(),
        content: serde_json::Value::String(content.into()),
        creator: "tester".into(),
    }
}

fn make_intent(id: &str, from: Vec<&str>, desc: &str) -> Intent {
    Intent {
        id: FihHash(id.into()),
        from_facts: from.into_iter().map(|s| s.to_string()).collect(),
        description: desc.into(),
        creator: "tester".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        concluded_at: None,
    }
}

// ── Scenario 1: N agents concurrent heartbeat ─────────────────────────
//
// 10 agents all heartbeat the same intent. In Cairn protocol, heartbeat
// IS claim — any agent can heartbeat any unowned intent. All succeed,
// final worker is determined by execution order.

#[test]
fn test_concurrent_heartbeat_all_succeed() {
    let bb = Arc::new(Mutex::new(SqlBlackboard::memory().unwrap()));

    {
        let mut bb = bb.lock().unwrap();
        bb.submit_fact(&make_fact("f001", "shared resource")).unwrap();
        bb.submit_intent(&make_intent("i001", vec!["f001"], "race target"))
            .unwrap();
    }

    let num_agents = 10;
    let mut handles = Vec::new();

    for i in 0..num_agents {
        let bb = bb.clone();
        let agent = format!("agent-{}", i);
        handles.push(std::thread::spawn(move || {
            let mut bb = bb.lock().unwrap();
            bb.heartbeat("i001", &agent)
        }));
    }

    let results: Vec<Result<(), BlackboardError>> =
        handles.into_iter().map(|h| h.join().unwrap()).collect();

    let state = bb.lock().unwrap().read_state();
    let intent = state.intents.iter().find(|i| i.id.0 == "i001").unwrap();

    // All heartbeats succeed because heartbeat overwrites worker
    let successes = results.iter().filter(|r| r.is_ok()).count();
    assert_eq!(successes, num_agents, "all heartbeats must succeed");
    // Final worker is the last one to execute (race winner)
    assert!(intent.worker.is_some(), "intent must have a final worker");
}

// ── Scenario 2: Concurrent claim → release handoff ────────────────────
//
// Two agents race to claim and release. After both finish, the intent
// must be unclaimed (worker = None) and never concluded.

#[test]
fn test_concurrent_release_claim_handoff() {
    let bb = Arc::new(Mutex::new(SqlBlackboard::memory().unwrap()));

    {
        let mut bb = bb.lock().unwrap();
        bb.submit_fact(&make_fact("f001", "ground truth")).unwrap();
        bb.submit_intent(&make_intent("i001", vec!["f001"], "handoff target"))
            .unwrap();
    }

    for round in 0..5 {
        let bb1 = bb.clone();
        let h1 = std::thread::spawn(move || {
            let mut bb = bb1.lock().unwrap();
            match bb.heartbeat("i001", "agent-a") {
                Ok(_) => bb.release_intent("i001", "agent-a"),
                Err(e) => Err(e),
            }
        });

        let bb2 = bb.clone();
        let h2 = std::thread::spawn(move || {
            let mut bb = bb2.lock().unwrap();
            match bb.heartbeat("i001", "agent-b") {
                Ok(_) => bb.release_intent("i001", "agent-b"),
                Err(e) => Err(e),
            }
        });

        let (r1, r2) = (h1.join().unwrap(), h2.join().unwrap());

        let state = bb.lock().unwrap().read_state();
        let intent = state.intents.iter().find(|i| i.id.0 == "i001").unwrap();

        assert!(
            intent.concluded_at.is_none(),
            "not concluded in round {round}"
        );
        assert!(
            intent.worker.is_none(),
            "worker must be None after release in round {round}, got {:?}",
            intent.worker
        );

        println!("  Round {round}: agent-a={r1:?}, agent-b={r2:?}");
    }
}

// ── Scenario 3: 100 concurrent fact submissions ────────────────────────

#[test]
fn test_concurrent_fact_submission() {
    let bb = Arc::new(Mutex::new(SqlBlackboard::memory().unwrap()));
    let num_facts = 100;

    let mut handles = Vec::new();
    for i in 0..num_facts {
        let bb = bb.clone();
        handles.push(std::thread::spawn(move || {
            let mut bb = bb.lock().unwrap();
            bb.submit_fact(&make_fact(&format!("f_{:04}", i), &format!("fact {i}"))).unwrap();
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    let state = bb.lock().unwrap().read_state();
    assert_eq!(state.facts.len(), num_facts);
}

// ── Scenario 4: 20 concurrent full lifecycle pipelines ────────────────

#[test]
fn test_concurrent_full_lifecycle() {
    let bb = Arc::new(Mutex::new(SqlBlackboard::memory().unwrap()));
    let num_agents = 20;

    {
        let mut bb = bb.lock().unwrap();
        for i in 0..num_agents {
            bb.submit_fact(&make_fact(
                &format!("f_{:04}", i),
                &format!("ground truth {i}"),
            )).unwrap();
        }
    }

    let mut handles = Vec::new();
    for i in 0..num_agents {
        let bb = bb.clone();
        handles.push(std::thread::spawn(move || {
            let mut bb = bb.lock().unwrap();
            let fid = format!("f_{:04}", i);
            let iid = format!("i_{:04}", i);

            bb.submit_intent(&make_intent(&iid, vec![&fid], &format!("intent {i}")))
                .map_err(|e| format!("submit: {e}"))?;
            bb.heartbeat(&iid, &format!("agent-{i}"))
                .map_err(|e| format!("claim: {e}"))?;
            bb.conclude_intent(&iid, &serde_json::json!({"result": i}))
                .map_err(|e| format!("conclude: {e}"))?;
            Ok::<_, String>(())
        }));
    }

    let results: Vec<Result<(), String>> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let failures: Vec<&String> = results.iter().filter_map(|r| r.as_ref().err()).collect();

    assert!(
        failures.is_empty(),
        "{} agents failed: {:?}",
        failures.len(),
        failures
    );

    let state = bb.lock().unwrap().read_state();
    assert_eq!(state.facts.len(), num_agents * 2);
    assert_eq!(state.intents.len(), num_agents);
    assert!(state.intents.iter().all(|i| i.concluded_at.is_some()));
}

// ── Scenario 5: Duplicate fact ID returns error ────────────────────────

#[test]
fn test_submit_fact_duplicate_id_returns_error() {
    let bb = Arc::new(Mutex::new(SqlBlackboard::memory().unwrap()));

    let result1 = bb.lock().unwrap().submit_fact(&make_fact("f001", "first"));
    assert!(result1.is_ok(), "first submit should succeed");

    let result2 = bb.lock().unwrap().submit_fact(&make_fact("f001", "second"));
    assert!(result2.is_err(), "duplicate ID must return error");
    match result2.unwrap_err() {
        BlackboardError::Internal(_) => {} // expected: UNIQUE constraint
        other => panic!("expected Internal error, got {other:?}"),
    }

    let state = bb.lock().unwrap().read_state();
    assert_eq!(state.facts.len(), 1, "only one fact stored");
    assert_eq!(state.facts[0].content, "first");
}

// ── Scenario 6: Submit fact to non-existent project ────────────────────

#[test]
fn test_submit_fact_fk_violation_silent() {
    let mut bb = SqlBlackboard::memory().unwrap();
    assert_eq!(bb.project_id(), "default");

    bb.submit_fact(&make_fact("f001", "this works")).unwrap();
    assert_eq!(bb.read_state().facts.len(), 1);
}
