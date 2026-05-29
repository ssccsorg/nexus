// Gateway-level tests exercising DefaultBlackboard with IoBufferSession
// as the cold storage backend.
//
// These tests simulate the rs-worker consumer pattern:
//   1. Instantiate IoBufferSession (the in-memory working set)
//   2. Wire its CompositeColdStorage through DefaultBlackboard
//   3. Verify sync operations produce correct dirty tracking

use nexus_graph::create_blackboard_with_storage;
use nexus_model::{
    Blackboard, Fact, FihHash, FlushCapable, FlushCursor, Hint, Intent,
};
use nexus_storage_kv_cold::{IoBufferSession};
use nexus_storage_petgraph::PetgraphStorage;

// ── Helpers ──────────────────────────────────────────────────────────────

fn test_fact(id: &str) -> Fact {
    Fact {
        id: FihHash(id.to_string()),
        origin: "gateway-test".into(),
        content: serde_json::json!({"value": id}),
        creator: "gateway-agent".into(),
    }
}

fn test_intent(id: &str) -> Intent {
    Intent {
        id: FihHash(id.to_string()),
        from_facts: vec![format!("{id}_ground")],
        to_fact_id: None,
        description: format!("intent {id}"),
        creator: "gateway-agent".into(),
        worker: None,
        last_heartbeat_at: None,
        created_at: Some("0".into()),
        concluded_at: None,
    }
}

fn test_hint(id: &str) -> Hint {
    Hint {
        id: FihHash(id.to_string()),
        content: format!("hint {id}"),
        creator: "gateway-agent".into(),
    }
}

/// Build a blackboard with PetgraphStorage (hot) + IoBufferSession (cold).
///
/// The session's CompositeColdStorage is used as the cold backend.
/// This mirrors the rs-worker pattern: IoBufferSession holds the working
/// copy, while the blackboard provides the full FIH lifecycle.
fn make_bb() -> (IoBufferSession, impl Blackboard + FlushCapable) {
    let session = IoBufferSession::new("gw-test-project");
    let cold = session.storage().clone(); // CompositeColdStorage
    let hot = PetgraphStorage::with_project_id("gw-test-project");
    let bb = create_blackboard_with_storage(hot, Box::new(cold));
    (session, bb)
}

// ─── Submit / read ──────────────────────────────────────────────────────

#[test]
fn test_gateway_submit_fact_dirty() {
    let (session, mut bb) = make_bb();
    bb.submit_fact(&test_fact("gw_f1")).unwrap();

    let state = Blackboard::read_state(&bb);
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.facts[0].id.0, "gw_f1");

    let kv_puts = session.drain_kv_puts();
    assert!(!kv_puts.is_empty());
    assert!(kv_puts.iter().any(|(_, v)| v.contains("gw_f1")));
}

// ─── Claim / conflict ───────────────────────────────────────────────────

#[test]
fn test_gateway_claim_and_dirty() {
    let (session, mut bb) = make_bb();
    session.storage().submit_fact(&test_fact("ground")).unwrap();
    session.storage().submit_intent(&test_intent("c1")).unwrap();
    let _ = session.drain_kv_puts();

    bb.claim_intent("c1", "gw-w1").expect("claim");
    let kv = session.drain_kv_puts();
    let obj = session.drain_object_puts();
    assert!(!kv.is_empty(), "claim produces KV dirty");
    assert!(!obj.is_empty(), "claim produces CAS dirty");
}

#[test]
fn test_gateway_double_claim_fails() {
    let (session, mut bb) = make_bb();
    session.storage().submit_fact(&test_fact("g")).unwrap();
    session.storage().submit_intent(&test_intent("dc")).unwrap();
    let _ = session.drain_kv_puts();
    bb.claim_intent("dc", "a").unwrap();
    assert!(bb.claim_intent("dc", "b").is_err());
}

// ─── Conclude ───────────────────────────────────────────────────────────

#[test]
fn test_gateway_conclude_channel_dirty() {
    let (session, mut bb) = make_bb();
    session.storage().submit_fact(&test_fact("g")).unwrap();
    session.storage().submit_intent(&test_intent("cc")).unwrap();
    bb.claim_intent("cc", "w").unwrap();
    let _ = session.drain_kv_puts();
    let _ = session.drain_object_puts();

    bb.conclude_intent("cc", &serde_json::json!({"ok": true})).unwrap();

    let del = session.drain_kv_deletes();
    let put = session.drain_kv_puts();
    assert!(!del.is_empty(), "conclude deletes intent");
    assert!(!put.is_empty(), "conclude creates fact");
    assert!(del.iter().any(|k| k.contains("cc")));
}

// ─── Flush ──────────────────────────────────────────────────────────────

#[test]
fn test_gateway_flush_dirty() {
    let (session, mut bb) = make_bb();
    for i in 0..3 {
        session.storage().submit_fact(&test_fact(&format!("f{i}"))).unwrap();
    }
    let _ = session.drain_kv_puts();

    let cursor = FlushCursor {
        last_flushed_at: String::new(),
        partition: "p1".into(),
    };
    bb.flush_since(&cursor).unwrap();

    let blob = session.drain_blob_puts();
    let kv = session.drain_kv_puts();
    assert!(!blob.is_empty(), "flush produces blob dirty");
    assert!(!kv.is_empty(), "flush produces cursor dirty");
    assert!(kv.iter().any(|(k, _)| k.contains("cursor")));
}

#[test]
fn test_gateway_empty_flush_still_writes_cursor() {
    let (session, mut bb) = make_bb();
    let cursor = FlushCursor { last_flushed_at: String::new(), partition: "e".into() };
    bb.flush_since(&cursor).unwrap();
    assert!(session.drain_blob_puts().is_empty());
    assert!(!session.drain_kv_puts().is_empty());
}

// ─── Multi-entity lifecycle ─────────────────────────────────────────────

#[test]
fn test_gateway_multi_lifecycle_dirty() {
    let (session, mut bb) = make_bb();
    bb.submit_fact(&test_fact("mf")).unwrap();
    session.storage().submit_intent(&test_intent("mi")).unwrap();
    // Intent needs a pre-existing fact in hot storage — use bb for consistency
    // Actually submit via bb so hot sees it
    // Re-do: submit fact + intent both via bb so PetgraphStorage has them
    // But bb.submit_intent also goes through hot+cold, so it's fine.
    // (the session already has the intent via session.storage() directly above)
    // Let's use bb for both for clean consistency:
    let (session, mut bb) = make_bb();
    bb.submit_fact(&test_fact("mf")).unwrap();
    bb.submit_intent(&test_intent("mi")).unwrap();
    let _ = session.drain_kv_puts();

    bb.claim_intent("mi", "w").unwrap();
    bb.conclude_intent("mi", &serde_json::json!({"done": true})).unwrap();

    assert!(!session.drain_kv_deletes().is_empty());
    assert!(!session.drain_kv_puts().is_empty());
    assert!(!session.drain_object_puts().is_empty());
}

// ─── Read state ─────────────────────────────────────────────────────────

#[test]
fn test_gateway_read_state_reflects_session() {
    let (session, mut bb) = make_bb();
    session.storage().submit_fact(&test_fact("pre")).unwrap();
    let state = Blackboard::read_state(&bb);
    assert_eq!(state.facts.len(), 1);
}

// ─── Edge: large content ────────────────────────────────────────────────

#[test]
fn test_gateway_large_fact() {
    let (session, mut bb) = make_bb();
    let big = Fact {
        id: FihHash("big".into()),
        origin: "t".into(),
        content: serde_json::json!("x".repeat(100_000)),
        creator: "a".into(),
    };
    bb.submit_fact(&big).unwrap();
    let state = Blackboard::read_state(&bb);
    assert_eq!(state.facts.len(), 1);
}

// ─── Hint ───────────────────────────────────────────────────────────────

#[test]
fn test_gateway_hint_dirty() {
    let (session, mut bb) = make_bb();
    bb.submit_hint(&test_hint("h1")).unwrap();
    let kv = session.drain_kv_puts();
    assert!(!kv.is_empty());
}
