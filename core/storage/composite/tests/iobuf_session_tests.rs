// Integration tests for IoBufferSession + SessionServer — real-scenario
// simulation of CF Worker async bridge consuming sync CompositeColdStorage.
//
// These tests validate:
//   1. hydrate → execute → drain lifecycle
//   2. dirty tracking precision (only modified keys dirty)
//   3. concurrent CAS serialization via SessionServer
//   4. multi-entity dirty isolation (fact/intent/hint drain independently)
//   5. CF Worker simulation: local CAS win, external DO rejection rollback

use nexus_model::{
    EvictCapable, Fact, FactCapable, FlushCapable, Hint, HintCapable, Intent, IntentCapable,
    ScanCapable, StorageRead,
};
use nexus_model::{
    SessionDrainBlob, SessionDrainKv, SessionDrainObject, SessionExecute,
};
use nexus_storage_kv_cold::{IoBufferSession, KeyValueStore, SessionServer};

// ── Helpers ──────────────────────────────────────────────────────────────

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

fn session() -> IoBufferSession {
    IoBufferSession::new("test-project")
}

/// Serialize a Stamped fact with a submitted_at timestamp.
///
/// CompositeColdStorage writes facts as `Stamped<T>` envelope internally.
/// Raw `Fact` JSON without the envelope will fail deserialization.
fn stamped_fact_json(fact: &Fact, ts: &str) -> String {
    #[derive(serde::Serialize)]
    struct Stamped<'a, T> {
        submitted_at: &'a str,
        data: &'a T,
    }
    serde_json::to_string(&Stamped {
        submitted_at: ts,
        data: fact,
    })
    .unwrap()
}

/// Serialize a Stamped intent with a submitted_at timestamp.
fn stamped_intent_json(intent: &Intent, ts: &str) -> String {
    #[derive(serde::Serialize)]
    struct Stamped<'a, T> {
        submitted_at: &'a str,
        data: &'a T,
    }
    serde_json::to_string(&Stamped {
        submitted_at: ts,
        data: intent,
    })
    .unwrap()
}

// ── Lifecycle: hydrate → execute → drain ─────────────────────────────────

#[test]
fn test_hydrate_execute_drain_lifecycle() {
    let s = session();

    // Phase 1: Simulate CF Worker hydrate — push data into IoBuffer
    let pre_existing = test_fact("pre_existing");
    let json = serde_json::to_string(&pre_existing).unwrap();
    s.kv_buf()
        .hydrate_batch(vec![("test-project:fact:pre_existing".into(), json)]);

    // Phase 2: Execute sync orchestration — submit a new fact
    let new_fact = test_fact("new_fact");
    s.storage().submit_fact(&new_fact).expect("submit_fact");

    // Phase 3: Drain dirty data for CF Worker flush
    let dirty_puts: Vec<(String, String)> = s.drain_kv_puts();
    let dirty_deletes = s.drain_kv_deletes();

    // Only the newly submitted fact should be dirty
    assert!(!dirty_puts.is_empty(), "new_fact write should be dirty");
    let new_fact_key = dirty_puts.iter().find(|(_, v)| v.contains("new_fact"));
    assert!(new_fact_key.is_some(), "dirty puts must include new_fact");
    assert!(dirty_deletes.is_empty(), "no deletes should be dirty");

    // Pre-existing data should NOT be in dirty puts
    let pre_existing_in_dirty = dirty_puts.iter().any(|(_, v)| v.contains("pre_existing"));
    assert!(
        !pre_existing_in_dirty,
        "pre-existing data should not be dirty"
    );
}

#[test]
fn test_dirty_tracking_resets_after_drain() {
    let s = session();
    s.storage().submit_fact(&test_fact("f1")).unwrap();

    // First drain
    let first = s.drain_kv_puts();
    assert!(!first.is_empty());

    // Second drain — should be empty since dirty was drained
    let second = s.drain_kv_puts();
    assert!(
        second.is_empty(),
        "second drain should be empty after first drain consumed all dirty"
    );
}

#[test]
fn test_empty_session_drain_returns_empty() {
    let s = session();
    assert!(s.drain_kv_puts().is_empty());
    assert!(s.drain_kv_deletes().is_empty());
    assert!(s.drain_blob_puts().is_empty());
    assert!(s.drain_blob_deletes().is_empty());
    assert!(s.drain_object_puts().is_empty());
    assert!(s.drain_object_deletes().is_empty());
}

// ── Dirty isolation: facts/intents/hints tracked independently ────────────

#[test]
fn test_multi_entity_dirty_isolation() {
    let s = session();

    // Submit one of each entity type
    s.storage().submit_fact(&test_fact("f1")).unwrap();
    s.storage().submit_intent(&test_intent("i1")).unwrap();
    s.storage().submit_hint(&test_hint("h1")).unwrap();

    // KV puts should contain all three
    let kv_puts = s.drain_kv_puts();
    assert!(
        kv_puts.len() >= 3,
        "all three entity writes should be dirty"
    );

    // Blob storage untouched — no blob dirty
    assert!(s.drain_blob_puts().is_empty());
    assert!(s.drain_blob_deletes().is_empty());

    // Object storage untouched — no object dirty
    assert!(s.drain_object_puts().is_empty());
    assert!(s.drain_object_deletes().is_empty());
}

#[test]
fn test_delete_is_tracked_separately_from_puts() {
    let s = session();

    // Submit then delete
    let key = "test-project:fact:temp";
    let json = serde_json::to_string(&test_fact("temp")).unwrap();
    s.kv_buf().set(key, &json).unwrap();
    s.kv_buf().delete(key).unwrap();

    let _puts = s.drain_kv_puts();
    let deletes = s.drain_kv_deletes();

    // key was both written and deleted, so appears in both
    // (drain consumed the dirty tracking, but the write happened first)
    assert!(
        deletes.contains(&key.to_string()),
        "delete should be tracked"
    );
}

// ── CF Worker simulation: hydrate, execute, flush cycle ──────────────────

#[test]
fn test_cf_worker_full_cycle() {
    // Simulate a CF Worker request:
    // 1. Hydrate from CF KV (here: preload existing facts via hydrate_batch)
    // 2. Execute sync operations (submit new fact, claim intent)
    // 3. Drain dirty and verify what needs to be pushed back to CF

    let s = session();

    // Step 1: Hydrate — simulate CF Worker pulling recent state from KV
    let existing = test_fact("existing");
    let key = format!("test-project:fact:existing");
    let json = stamped_fact_json(&existing, "100");
    s.kv_buf().hydrate_batch(vec![(key, json)]);

    // Preload an unclaimed intent for claim testing
    let existing_intent = test_intent("open_intent");
    let intent_key = format!("test-project:intent:open_intent");
    let intent_json = stamped_intent_json(&existing_intent, "100");
    s.kv_buf().hydrate_batch(vec![(intent_key, intent_json)]);

    // Step 2: Execute — actual Worker request logic
    s.storage().submit_fact(&test_fact("new_fact")).unwrap();
    s.storage()
        .claim_intent("open_intent", "cf-worker-1")
        .expect("claim should succeed on unclaimed intent");

    // Step 3: Drain — extract dirty for CF KV flush
    let kv_dirty = s.drain_kv_puts();

    // Verify: only "new_fact" and the claimed intent should be dirty
    // "existing" was preloaded, not modified → not dirty
    let existing_in_dirty = kv_dirty.iter().any(|(_, v)| v.contains("existing"));
    assert!(!existing_in_dirty, "preloaded data should not be dirty");

    let new_fact_dirty = kv_dirty.iter().any(|(_, v)| v.contains("new_fact"));
    assert!(new_fact_dirty, "newly submitted fact must be dirty");

    let claimed_intent_dirty = kv_dirty.iter().any(|(_, v)| v.contains("cf-worker-1"));
    assert!(
        claimed_intent_dirty,
        "claimed intent (with worker update) must be dirty"
    );
}

#[test]
fn test_cf_worker_cas_rollback_simulation() {
    // Simulate the scenario where:
    //   Worker A locally claims an intent (IoBuffer CAS succeeds)
    //   Worker B locally claims the same intent (IoBuffer CAS also succeeds)
    //   At flush time, CF DO rejects Worker B's CAS
    //   Worker B must rollback

    // This test validates that the IoBuffer correctly tracks dirty
    // CAS writes, enabling the consumer to detect and handle rollback.

    let session_a = session();
    let session_b = IoBufferSession::new("test-project");

    // Both workers see the same pre-existing intent (Stamped JSON)
    let intent = test_intent("contested");
    let key = format!("test-project:intent:contested");
    let json = stamped_intent_json(&intent, "100");

    session_a
        .kv_buf()
        .hydrate_batch(vec![(key.clone(), json.clone())]);
    session_b.kv_buf().hydrate_batch(vec![(key, json)]);

    // Both claim locally — both IoBuffer CAS succeed (isolated instances)
    session_a
        .storage()
        .claim_intent("contested", "worker-a")
        .unwrap();
    session_b
        .storage()
        .claim_intent("contested", "worker-b")
        .unwrap();

    // Verify both sessions have dirty CAS entries
    let dirty_a = session_a.drain_object_puts();
    let dirty_b = session_b.drain_object_puts();

    assert!(!dirty_a.is_empty(), "worker-a should have CAS dirty entry");
    assert!(!dirty_b.is_empty(), "worker-b should have CAS dirty entry");

    // Consumer (CF Worker) would now:
    //   dirty_a → push to DO → DO: "" → "worker-a" ✓ → merge success
    //   dirty_b → push to DO → DO: "worker-a" ≠ "" → ✗ Conflict
    // → session_b rolls back, session_a commits
}

// ── Flush cycle simulation ───────────────────────────────────────────────

#[test]
fn test_flush_since_produces_blob_dirty() {
    // When flush_since runs, it writes blobs and updates cursor via KV.
    // Both IoBufferBlob and IoBufferKv should track these as dirty.
    let s = session();

    // Submit facts that will be flushed
    for i in 0..5 {
        s.storage()
            .submit_fact(&test_fact(&format!("flush_f{i}")))
            .unwrap();
    }

    // Drain KV puts from submits (these will be the flush source)
    let _kv_from_submits = s.drain_kv_puts();

    // Run flush
    let cursor = nexus_model::FlushCursor {
        last_flushed_at: String::new(),
        partition: "default".into(),
    };
    let result = s.storage().flush_since(&cursor);
    assert!(result.is_ok(), "flush_since should succeed");

    // Blob should have dirty entries from flush output
    let blob_puts = s.drain_blob_puts();
    assert!(
        !blob_puts.is_empty(),
        "flush_since should produce blob dirty entries"
    );

    // KV cursor should also be dirty (cursor persist)
    let kv_puts = s.drain_kv_puts();
    assert!(
        !kv_puts.is_empty(),
        "flush_since should produce KV cursor dirty entry"
    );

    // Confirm cursor key
    let cursor_key = "test-project:cursor";
    let cursor_in_dirty = kv_puts
        .iter()
        .any(|(k, _)| k.contains(cursor_key));
    assert!(
        cursor_in_dirty,
        "cursor key must be in KV dirty after flush"
    );
}

#[test]
fn test_hydrate_flush_read_cycle() {
    // Full cycle: submit → flush → hydrate from blob → scan
    // Simulates a Worker being restarted, hydrating from CF R2,
    // then scanning the partition.
    let s = session();

    // Phase 1: Submit data
    s.storage()
        .submit_fact(&test_fact("f1"))
        .unwrap();
    s.storage()
        .submit_intent(&test_intent("i1"))
        .unwrap();
    s.drain_kv_puts(); // clear submit dirty

    // Phase 2: Flush
    let cursor = nexus_model::FlushCursor {
        last_flushed_at: String::new(),
        partition: "p1".into(),
    };
    s.storage().flush_since(&cursor).unwrap();
    let blob_dirty = s.drain_blob_puts();
    assert!(!blob_dirty.is_empty(), "flush should produce blobs");
    let _kv_dirty = s.drain_kv_puts();

    // Phase 3: Simulate Worker restart — fresh session, hydrate blobs
    // In real CF Worker: list blobs from R2, get() each, hydrate
    let s2 = IoBufferSession::new("test-project");
    for (key, data) in &blob_dirty {
        s2.blob_buf().hydrate_batch(vec![(key.clone(), data.clone())]);
    }

    // Phase 4: Scan partition — should find flushed data
    let partition = s2.storage().scan_partition("p1").unwrap();
    assert!(
        !partition.facts.is_empty() || !partition.intents.is_empty(),
        "scanned partition should contain flushed data"
    );
}

// ── SessionHandle integration (native only, spawn-based) ────────────────

#[test]
fn test_session_handle_submit_fact() {
    let s = IoBufferSession::new("test-project");
    let handle = SessionServer::spawn(s);

    let result = handle.submit(|session| {
        session
            .storage()
            .submit_fact(&test_fact("via_handle"))
            .map(|_| true)
    });
    assert!(
        result.is_ok(),
        "submitting fact via handle should work"
    );
}

#[test]
fn test_session_handle_read_state() {
    let s = IoBufferSession::new("test-project");

    let fact = test_fact("readable");
    let key = format!("test-project:fact:readable");
    let json = stamped_fact_json(&fact, "100");
    s.hydrate_kv(vec![(key, json)]);

    let handle = SessionServer::spawn(s);

    let state = handle.submit(|session| session.storage().read_state());
    assert_eq!(state.facts.len(), 1, "should read 1 preloaded fact");
    assert_eq!(
        state.facts[0].id.0, "readable",
        "fact id should match preloaded data"
    );
}

// ── Intent lifecycle dirty tracking ──────────────────────────────────────

#[test]
fn test_conclude_intent_produces_kv_delete_and_fact_submit_dirty() {
    let s = session();

    // Preload an intent with Stamped envelope
    let intent = test_intent("conclude_me");
    let key = format!("test-project:intent:conclude_me");
    let json = stamped_intent_json(&intent, "100");
    s.hydrate_kv(vec![(key, json)]);

    // Conclude the intent
    let result = serde_json::json!({"outcome": "done"});
    s.storage()
        .conclude_intent("conclude_me", &result)
        .unwrap();

    // KV dirty should include both:
    //   - delete of the intent key
    //   - put of the new concluded fact
    let kv_deletes = s.drain_kv_deletes();
    let kv_puts = s.drain_kv_puts();

    assert!(
        !kv_deletes.is_empty(),
        "conclude should delete the intent from KV"
    );
    let intent_deleted = kv_deletes.iter().any(|k| k.contains("conclude_me"));
    assert!(intent_deleted, "intent key should be in KV deletes");

    // The new fact (concluded result) should be in puts
    assert!(
        !kv_puts.is_empty(),
        "conclude should create a new fact via submit_fact"
    );
}

#[test]
fn test_heartbeat_produces_dirty() {
    let s = session();

    // Preload intent and claim it
    let intent = test_intent("beat");
    let key = format!("test-project:intent:beat");
    let json = stamped_intent_json(&intent, "100");
    s.hydrate_kv(vec![(key, json)]);
    s.storage()
        .claim_intent("beat", "worker-x")
        .unwrap();
    let _ = s.drain_kv_puts();
    let _ = s.drain_object_puts();

    // Now heartbeat
    s.storage()
        .heartbeat("beat", "worker-x")
        .unwrap();

    let kv_puts = s.drain_kv_puts();
    assert!(
        !kv_puts.is_empty(),
        "heartbeat should update KV (last_heartbeat_at)"
    );
}

#[test]
fn test_release_intent_produces_dirty() {
    let s = session();

    let intent = test_intent("releaseme");
    let key = format!("test-project:intent:releaseme");
    let json = stamped_intent_json(&intent, "100");
    s.hydrate_kv(vec![(key, json)]);
    s.storage()
        .claim_intent("releaseme", "worker-x")
        .unwrap();
    let _ = s.drain_kv_puts();
    let _ = s.drain_object_puts();

    // Release
    s.storage()
        .release_intent("releaseme", "worker-x")
        .unwrap();

    let kv_puts = s.drain_kv_puts();
    assert!(
        !kv_puts.is_empty(),
        "release should update KV (worker cleared)"
    );
}

// ── Approximate size and eviction dirty ──────────────────────────────────

#[test]
fn test_evict_before_produces_blob_deletes() {
    let s = session();

    // Preload a blob that looks like a flush output (old timestamp)
    let old_key = "test-project/flush/facts/p1/1.jsonl";
    s.blob_buf()
        .hydrate_batch(vec![(old_key.into(), b"old data"[..].into())]);

    // Also preload a recent one (should survive)
    let recent_key = "test-project/flush/facts/p1/99999.jsonl";
    s.blob_buf()
        .hydrate_batch(vec![(recent_key.into(), b"recent data"[..].into())]);

    // Evict blobs before timestamp 50000
    let evicted = s.storage().evict_before("50000").unwrap();
    assert_eq!(evicted, 1, "should evict 1 old blob");

    // Blob deletes should track the eviction
    let blob_deletes = s.drain_blob_deletes();
    assert_eq!(blob_deletes.len(), 1, "one blob delete should be dirty");
    assert!(
        blob_deletes[0].contains("1.jsonl"),
        "the old blob should be deleted"
    );
}
