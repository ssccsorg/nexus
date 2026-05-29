// Integration tests for CompositeColdStorage with MockKv + MockBlob + MockObject.

use nexus_model::{
    BlackboardError, EvictCapable, Fact, FactCapable, FilterCapable, FlushCapable, FlushCursor,
    Hint, HintCapable, Intent, IntentCapable, ScanCapable, StateFilter, StorageRead,
};
use nexus_storage_composite::{BlobStore, CompositeColdStorage, KeyValueStore, ObjectStore};

mod common;
use common::{MockBlob, MockKv, MockObject};
use std::sync::{Arc, Barrier};

// ── Helpers ────────────────────────────────────────────────────────────────

fn test_fact(id: &str) -> Fact {
    Fact {
        id: nexus_model::FihHash(id.to_string()),
        origin: "test".into(),
        content: serde_json::json!({"value": id}),
        creator: "tester".into(),
    }
}

fn test_intent(id: &str) -> Intent {
    Intent {
        id: nexus_model::FihHash(id.to_string()),
        from_facts: vec![],
        to_fact_id: None,
        description: format!("intent {id}"),
        creator: "tester".into(),
        worker: None,
        last_heartbeat_at: None,
        created_at: Some("0".into()),
        concluded_at: None,
    }
}

fn test_hint(id: &str) -> Hint {
    Hint {
        id: nexus_model::FihHash(id.to_string()),
        content: format!("hint {id}"),
        creator: "tester".into(),
    }
}

fn storage() -> CompositeColdStorage<MockKv, MockBlob, MockObject> {
    CompositeColdStorage::new_with_system_clock(
        MockKv::new(),
        MockBlob::new(),
        MockObject::new(),
        "test-project",
    )
}

/// Create storage pre-populated with `n` facts.
fn storage_with_facts(n: usize) -> CompositeColdStorage<MockKv, MockBlob, MockObject> {
    let s = storage();
    for i in 0..n {
        s.submit_fact(&test_fact(&format!("f_{i}")))
            .expect("submit fact");
    }
    s
}

/// Create storage pre-populated with `n` intents.
fn storage_with_intents(n: usize) -> CompositeColdStorage<MockKv, MockBlob, MockObject> {
    let s = storage();
    for i in 0..n {
        s.submit_intent(&test_intent(&format!("int_{i}")))
            .expect("submit intent");
    }
    s
}

// ── StorageRead tests ──────────────────────────────────────────────────────

#[test]
fn test_empty_storage() {
    let s = storage();
    let state = s.read_state();
    assert!(state.facts.is_empty(), "no facts");
    assert!(state.intents.is_empty(), "no intents");
    assert!(state.hints.is_empty(), "no hints");
}

#[test]
fn test_project_id() {
    let s = storage();
    assert_eq!(s.project_id(), "test-project");
}

#[test]
fn test_fact_storage() {
    let s = storage_with_facts(3);
    let state = s.read_state();
    assert_eq!(state.facts.len(), 3);
    assert!(state.facts.iter().any(|f| f.id.0 == "f_0"));
    assert!(state.facts.iter().any(|f| f.id.0 == "f_2"));
}

#[test]
fn test_intent_storage() {
    let s = storage_with_intents(3);
    let state = s.read_state();
    assert_eq!(state.intents.len(), 3);
}

#[test]
fn test_hint_storage() {
    let s = storage();
    for i in 0..3 {
        s.submit_hint(&test_hint(&format!("h_{i}")))
            .expect("submit hint");
    }
    let state = s.read_state();
    assert_eq!(state.hints.len(), 3);
}

// ── FactCapable tests ─────────────────────────────────────────────────────

#[test]
fn test_submit_fact_returns_id() {
    let s = storage();
    let f = test_fact("f_hello");
    let id = s.submit_fact(&f).expect("submit");
    assert_eq!(id.0, "f_hello");
}

#[test]
fn test_duplicate_fact_allowed() {
    let s = storage();
    let f1 = test_fact("f_dup");
    let f2 = test_fact("f_dup");
    s.submit_fact(&f1).expect("first submit");
    s.submit_fact(&f2).expect("second submit (same id)");
    let state = s.read_state();
    assert!(state.facts.iter().filter(|f| f.id.0 == "f_dup").count() >= 1);
}

// ── IntentCapable tests ───────────────────────────────────────────────────

#[test]
fn test_intent_lifecycle() {
    let s = storage_with_intents(1);
    let intent_id = "int_0";

    // Claim
    s.claim_intent(intent_id, "agent-a").expect("claim");

    // Verify claimed
    let state = s.read_state();
    let intent = state.intents.iter().find(|i| i.id.0 == intent_id).unwrap();
    assert_eq!(intent.worker, Some("agent-a".into()));
    assert!(intent.last_heartbeat_at.is_some());

    // Heartbeat
    s.heartbeat(intent_id, "agent-a").expect("heartbeat");

    // Wrong agent heartbeat
    match s.heartbeat(intent_id, "agent-b") {
        Err(BlackboardError::Conflict(_)) => {}
        other => panic!("expected Conflict, got {other:?}"),
    }

    // Release
    s.release_intent(intent_id, "agent-a").expect("release");

    // Verify released
    let state = s.read_state();
    let intent = state.intents.iter().find(|i| i.id.0 == intent_id).unwrap();
    assert_eq!(intent.worker, None);

    // Conclude
    let result = serde_json::json!({"finding": "test result"});
    let fact = s.conclude_intent(intent_id, &result).expect("conclude");
    assert_eq!(fact.origin, format!("intent:{intent_id}"));

    // Intent is no longer in KV after conclusion
    let state = s.read_state();
    assert!(!state.intents.iter().any(|i| i.id.0 == intent_id));

    // Fact was created by conclusion
    assert!(
        state
            .facts
            .iter()
            .any(|f| f.origin == format!("intent:{intent_id}"))
    );
}

#[test]
fn test_claim_conflict() {
    let s = storage_with_intents(1);
    s.claim_intent("int_0", "agent-a").expect("first claim");
    match s.claim_intent("int_0", "agent-b") {
        Err(BlackboardError::Conflict(_)) => {}
        other => panic!("expected Conflict, got {other:?}"),
    }
}

#[test]
fn test_claim_nonexistent() {
    let s = storage();
    match s.claim_intent("nonexistent", "agent-a") {
        Err(BlackboardError::NotFound(_)) => {}
        other => panic!("expected NotFound, got {other:?}"),
    }
}

// ── HintCapable tests ─────────────────────────────────────────────────────

#[test]
fn test_submit_hint() {
    let s = storage();
    s.submit_hint(&test_hint("h_0")).expect("submit hint");
    let state = s.read_state();
    assert_eq!(state.hints.len(), 1);
    assert_eq!(state.hints[0].id.0, "h_0");
}

// ── FilterCapable tests ───────────────────────────────────────────────────

#[test]
fn test_filter_by_fact_ids() {
    let s = storage_with_facts(5);
    let filter = StateFilter {
        fact_ids: Some(vec!["f_1".into(), "f_3".into()]),
        ..StateFilter::default()
    };
    let state = s.read_state_filtered(&filter);
    assert_eq!(state.facts.len(), 2);
    assert!(state.facts.iter().any(|f| f.id.0 == "f_1"));
    assert!(state.facts.iter().any(|f| f.id.0 == "f_3"));
}

#[test]
fn test_filter_empty() {
    let s = storage_with_facts(5);
    let filter = StateFilter::default();
    let state = s.read_state_filtered(&filter);
    assert_eq!(state.facts.len(), 5);
}

#[test]
fn test_filter_by_intent_ids() {
    let s = storage_with_intents(5);
    let filter = StateFilter {
        intent_ids: Some(vec!["int_2".into()]),
        ..StateFilter::default()
    };
    let state = s.read_state_filtered(&filter);
    assert_eq!(state.intents.len(), 1);
    assert_eq!(state.intents[0].id.0, "int_2");
}

// ── ScanCapable tests ─────────────────────────────────────────────────────

#[test]
fn test_scan_partition_empty() {
    let s = storage();
    let data = s.scan_partition("test-partition").expect("scan");
    assert_eq!(data.partition, "test-partition");
    assert!(data.facts.is_empty());
    assert!(data.intents.is_empty());
    assert!(data.hints.is_empty());
}

#[test]
fn test_scan_partition_with_data() {
    let s = storage_with_facts(3);
    let data = s.scan_partition("test-partition").expect("scan");
    assert_eq!(data.facts.len(), 3);
}

// ── FlushCapable tests ────────────────────────────────────────────────────

#[test]
fn test_flush_empty_storage() {
    let s = storage();
    let cursor = FlushCursor::default();
    let result = s.flush_since(&cursor).expect("flush");
    assert_eq!(result.records_flushed, 0);
    assert!(!result.new_cursor.last_flushed_at.is_empty());
}

#[test]
fn test_flush_persists_data_to_blob() {
    let s = storage_with_facts(3);

    let cursor = FlushCursor::default();
    let result = s.flush_since(&cursor).expect("flush");
    assert_eq!(result.records_flushed, 3, "all facts flushed");

    // Blob store should contain the flushed data.
    let blob_keys = s.blob().list("").expect("list blobs");
    assert!(!blob_keys.is_empty(), "blobs exist after flush");
    let fact_blobs: Vec<_> = blob_keys.iter().filter(|k| k.contains("/facts/")).collect();
    assert_eq!(fact_blobs.len(), 1, "one facts blob");

    // Verify blob content is valid JSON lines.
    let blob = s.blob().get(fact_blobs[0]).expect("get blob");
    let content = String::from_utf8(blob.unwrap()).expect("utf8");
    assert!(content.contains("f_0"), "blob contains f_0");
    assert!(content.contains("f_2"), "blob contains f_2");
}

#[test]
fn test_cursor_persisted_to_kv() {
    let s = storage_with_facts(2);

    let cursor = FlushCursor::default();
    let result = s.flush_since(&cursor).expect("flush");

    // Cursor should be in KV.
    let cursor_key = format!("{}:cursor", s.project_id());
    let cursor_json = s
        .kv()
        .get(&cursor_key)
        .expect("get cursor")
        .expect("cursor exists");
    let restored: FlushCursor = serde_json::from_str(&cursor_json).expect("parse cursor");
    assert_eq!(restored.last_flushed_at, result.new_cursor.last_flushed_at);
}

#[test]
fn test_incremental_flush() {
    let s = storage();

    // First flush: submit 2 facts, flush.
    s.submit_fact(&test_fact("f_a")).expect("submit");
    s.submit_fact(&test_fact("f_b")).expect("submit");

    let cursor = FlushCursor::default();
    let r1 = s.flush_since(&cursor).expect("first flush");
    assert_eq!(r1.records_flushed, 2, "first flush: 2 facts");

    // Second flush: add 1 more fact, flush since previous cursor.
    s.submit_fact(&test_fact("f_c")).expect("submit");

    let cursor2 = FlushCursor {
        last_flushed_at: r1.new_cursor.last_flushed_at.clone(),
        partition: "default".into(),
    };
    let r2 = s.flush_since(&cursor2).expect("second flush");
    assert_eq!(r2.records_flushed, 1, "incremental flush: 1 new fact");

    // Cursor advanced.
    assert!(
        r2.new_cursor.last_flushed_at > r1.new_cursor.last_flushed_at,
        "cursor advanced"
    );
}

#[test]
fn test_flush_with_partition() {
    let s = storage();
    s.submit_fact(&test_fact("f_p")).expect("submit");

    let cursor = FlushCursor {
        last_flushed_at: String::new(),
        partition: "partition-x".into(),
    };
    let result = s.flush_since(&cursor).expect("flush with partition");
    assert_eq!(result.new_cursor.partition, "partition-x");

    // Blob should be under the partition prefix.
    let blob_keys = s.blob().list("").expect("list blobs");
    assert!(
        blob_keys[0].contains("partition-x"),
        "partition in blob key"
    );
}

#[test]
fn test_flush_does_not_remove_kv_facts() {
    let s = storage_with_facts(2);

    let state_before = s.read_state();
    let cursor = FlushCursor::default();
    s.flush_since(&cursor).expect("flush");
    let state_after = s.read_state();

    // Facts should still be readable from KV after flush.
    assert_eq!(state_before.facts.len(), state_after.facts.len());
}

// ── EvictCapable tests ────────────────────────────────────────────────────

#[test]
fn test_approximate_size_increases_with_data() {
    let s = storage();
    let empty_size = s.approximate_size();
    let s2 = storage_with_facts(3);
    let filled_size = s2.approximate_size();
    assert!(filled_size > empty_size, "size grows with data");
}

#[test]
fn test_evict_before_removes_old_blobs() {
    let s = storage_with_facts(3);
    let cursor = FlushCursor::default();
    s.flush_since(&cursor).expect("flush");
    let blob_count_before = s.blob().list("").expect("list").len();

    // Evict with a future timestamp — should remove all blobs.
    // Use a value larger than any nanosecond timestamp (~1.7e18).
    let future_ts = "9999999999999999999";
    let evicted = s.evict_before(future_ts).expect("evict");
    assert_eq!(evicted, blob_count_before as u64, "all blobs evicted");
    let blob_count_after = s.blob().list("").expect("list").len();
    assert_eq!(blob_count_after, 0, "no blobs remain");
}

#[test]
fn test_evict_before_keeps_recent_blobs() {
    let s = storage_with_facts(3);
    let cursor = FlushCursor::default();
    s.flush_since(&cursor).expect("flush");

    // Evict with timestamp 0 — should keep everything.
    let evicted = s.evict_before("0").expect("evict");
    assert_eq!(evicted, 0, "no blobs evicted");
}

// ── Combined lifecycle tests ───────────────────────────────────────────────

#[test]
fn test_submit_flush_read_cycle() {
    let s = CompositeColdStorage::new_with_system_clock(
        MockKv::new(),
        MockBlob::new(),
        MockObject::new(),
        "cycle-test",
    );

    // Submit data
    s.submit_fact(&test_fact("f_x")).expect("submit fact");
    s.submit_intent(&test_intent("i_y")).expect("submit intent");
    s.submit_hint(&test_hint("h_z")).expect("submit hint");

    // Read from KV
    let state = s.read_state();
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.intents.len(), 1);
    assert_eq!(state.hints.len(), 1);

    // Flush
    let r = s.flush_since(&FlushCursor::default()).expect("flush");
    assert_eq!(r.records_flushed, 3);

    // Scan partition should include flushed data
    let data = s.scan_partition("default").expect("scan");
    assert_eq!(data.facts.len(), 1);
    assert_eq!(data.intents.len(), 1);
    assert_eq!(data.hints.len(), 1);
}

#[test]
fn test_empty_blob_list_does_not_crash() {
    let s = storage();
    let data = s.scan_partition("empty").expect("scan empty");
    assert!(data.facts.is_empty());
}

#[test]
fn test_conclude_intent_creates_fact_and_removes_intent() {
    let s = storage_with_intents(1);
    let result = serde_json::json!({"answer": 42});
    let fact = s.conclude_intent("int_0", &result).expect("conclude");

    assert_eq!(fact.origin, "intent:int_0");
    assert_eq!(fact.content, result);

    let state = s.read_state();
    assert!(!state.intents.iter().any(|i| i.id.0 == "int_0"));
    assert!(state.facts.iter().any(|f| f.origin == "intent:int_0"));
}

// ── ObjectStore tests ─────────────────────────────────────────────────────

#[test]
fn test_mock_object_cas_basic() {
    let obj = MockObject::new();

    // Key does not exist — get_state returns None.
    assert_eq!(obj.get_state("intent:test").unwrap(), None);

    // Key matches "" (default) → put_state succeeds.
    let ok = obj.put_state("intent:test", "", "agent-a").unwrap();
    assert!(ok, "first put_state on empty key should succeed");
    assert_eq!(
        obj.get_state("intent:test").unwrap(),
        Some("agent-a".into())
    );

    // Key has "agent-a" — put_state with "" fails.
    let ok = obj.put_state("intent:test", "", "agent-b").unwrap();
    assert!(!ok, "second put_state should fail — state is agent-a");
    assert_eq!(
        obj.get_state("intent:test").unwrap(),
        Some("agent-a".into())
    );

    // Ownership transfer: put_state with correct expected.
    let ok = obj.put_state("intent:test", "agent-a", "agent-b").unwrap();
    assert!(ok, "ownership transfer put_state should succeed");
    assert_eq!(
        obj.get_state("intent:test").unwrap(),
        Some("agent-b".into())
    );

    // Independent keys are isolated.
    let ok = obj.put_state("intent:other", "", "agent-x").unwrap();
    assert!(ok, "independent key put_state should succeed");
    assert_eq!(
        obj.get_state("intent:test").unwrap(),
        Some("agent-b".into()),
        "other key does not affect test key"
    );

    // Release: put_state to "" removes the key.
    let ok = obj.put_state("intent:test", "agent-b", "").unwrap();
    assert!(ok, "release put_state should succeed");
    assert_eq!(
        obj.get_state("intent:test").unwrap(),
        None,
        "released key returns None"
    );
}

#[test]
fn test_concurrent_claim_with_cas_exactly_one_succeeds() {
    // Two threads claim the same intent simultaneously.
    // With ObjectStore CAS integrated into claim_intent, exactly one
    // worker should succeed and the other should get Conflict.
    let s = Arc::new(storage_with_intents(1));
    let barrier = Arc::new(Barrier::new(2));

    let s_a = Arc::clone(&s);
    let b_a = Arc::clone(&barrier);
    let h_a = std::thread::spawn(move || {
        b_a.wait();
        s_a.claim_intent("int_0", "agent-a")
    });

    let s_b = Arc::clone(&s);
    let b_b = Arc::clone(&barrier);
    let h_b = std::thread::spawn(move || {
        b_b.wait();
        s_b.claim_intent("int_0", "agent-b")
    });

    let r_a = h_a.join().unwrap();
    let r_b = h_b.join().unwrap();

    // With CAS: exactly one Ok, exactly one Conflict.
    let ok_count = [&r_a, &r_b].iter().filter(|r| matches!(r, Ok(()))).count();
    let conflict_count = [&r_a, &r_b]
        .iter()
        .filter(|r| matches!(r, Err(BlackboardError::Conflict(_))))
        .count();
    assert_eq!(
        ok_count, 1,
        "exactly one claim must succeed with CAS, got {ok_count}"
    );
    assert_eq!(
        conflict_count, 1,
        "exactly one claim must get Conflict with CAS, got {conflict_count}"
    );

    // Verify exactly one agent holds the claim in KV.
    let state = s.read_state();
    let intent = state.intents.iter().find(|i| i.id.0 == "int_0").unwrap();
    assert!(intent.worker.is_some(), "intent is claimed");
}
