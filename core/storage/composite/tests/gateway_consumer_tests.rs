// Gateway-consumer scenario tests: DefaultBlackboard + IoBufferSession.
//
// These validate the rs-worker consumer contract:
//   IoBufferSession → CompositeColdStorage → DefaultBlackboard
//   → sync operations → drain dirty → flush

use nexus_model::{
    Blackboard, Fact, FactCapable, FihHash, FlushCapable, FlushCursor, Hint, HintCapable,
    Intent, IntentCapable, SessionDrainBlob, SessionDrainKv, SessionDrainObject, SessionExecute,
};
use nexus_graph::create_blackboard_with_storage;
use nexus_storage_petgraph::PetgraphStorage;
use nexus_storage_kv_cold::{IoBufferSession};

// ── Helpers ──────────────────────────────────────────────────────────────

fn test_fact(id: &str) -> Fact {
    Fact { id: FihHash(id.into()), origin: "t".into(),
           content: serde_json::json!({"v": id}), creator: "a".into() }
}
fn test_intent(id: &str) -> Intent {
    Intent { id: FihHash(id.into()), from_facts: vec![format!("{id}_ground")],
             to_fact_id: None, description: format!("i {id}"), creator: "a".into(),
             worker: None, last_heartbeat_at: None,
             created_at: Some("0".into()), concluded_at: None }
}
fn test_hint(id: &str) -> Hint {
    Hint { id: FihHash(id.into()), content: format!("h {id}"), creator: "a".into() }
}

/// Build blackboard with PetgraphStorage (hot) + IoBufferSession (cold).
fn make_bb() -> (IoBufferSession, impl Blackboard + FlushCapable) {
    let ss = IoBufferSession::new("gw");
    let cold = ss.storage().clone();
    let hot = PetgraphStorage::with_project_id("gw");
    let bb = create_blackboard_with_storage(hot, Box::new(cold));
    (ss, bb)
}

// ─── Submit / read ──────────────────────────────────────────────────────

#[test]
fn test_gw_submit_fact_dirty() {
    let (ss, mut bb) = make_bb();
    bb.submit_fact(&test_fact("f1")).unwrap();
    let s = Blackboard::read_state(&bb);
    assert_eq!(s.facts.len(), 1);
    assert!(!ss.drain_kv_puts().is_empty());
}

#[test]
fn test_gw_submit_hint_dirty() {
    let (ss, mut bb) = make_bb();
    bb.submit_hint(&test_hint("h1")).unwrap();
    assert!(!ss.drain_kv_puts().is_empty());
}

// ─── Claim / double-claim / conclude ────────────────────────────────────

#[test]
fn test_gw_claim_and_conclude_dirty() {
    let (ss, mut bb) = make_bb();
    bb.submit_fact(&test_fact("c1_ground")).unwrap();
    bb.submit_intent(&test_intent("c1")).unwrap();
    let _ = ss.drain_kv_puts();

    bb.claim_intent("c1", "w1").unwrap();
    assert!(!ss.drain_kv_puts().is_empty(), "KV dirty after claim");
    assert!(!ss.drain_object_puts().is_empty(), "CAS dirty after claim");
}

#[test]
fn test_gw_double_claim_fails() {
    let (ss, mut bb) = make_bb();
    bb.submit_fact(&test_fact("dc_ground")).unwrap();
    bb.submit_intent(&test_intent("dc")).unwrap();
    let _ = ss.drain_kv_puts();
    bb.claim_intent("dc", "a").unwrap();
    assert!(bb.claim_intent("dc", "b").is_err());
}

#[test]
fn test_gw_conclude_delete_and_put() {
    let (ss, mut bb) = make_bb();
    bb.submit_fact(&test_fact("cc_ground")).unwrap();
    bb.submit_intent(&test_intent("cc")).unwrap();
    bb.claim_intent("cc", "w").unwrap();
    let _ = ss.drain_kv_puts();
    let _ = ss.drain_object_puts();

    bb.conclude_intent("cc", &serde_json::json!({})).unwrap();
    assert!(!ss.drain_kv_deletes().is_empty(), "conclude deletes");
    assert!(!ss.drain_kv_puts().is_empty(), "conclude puts fact");
}

// ─── Flush ──────────────────────────────────────────────────────────────

#[test]
fn test_gw_flush_dirty() {
    let (ss, mut bb) = make_bb();
    for i in 0..3 {
        ss.storage().submit_fact(&test_fact(&format!("f{i}"))).unwrap();
    }
    let _ = ss.drain_kv_puts();

    let c = FlushCursor { last_flushed_at: String::new(), partition: "p1".into() };
    bb.flush_since(&c).unwrap();

    assert!(!ss.drain_blob_puts().is_empty(), "flush produces blob");
    assert!(ss.drain_kv_puts().iter().any(|(k,_)|k.contains("cursor")), "flush writes cursor");
}

#[test]
fn test_gw_empty_flush_still_writes_cursor() {
    let (ss, mut bb) = make_bb();
    let c = FlushCursor { last_flushed_at: String::new(), partition: "e".into() };
    bb.flush_since(&c).unwrap();
    assert!(ss.drain_blob_puts().is_empty(), "no data → no blob");
    assert!(!ss.drain_kv_puts().is_empty(), "cursor always persisted");
}

// ─── Multi-entity lifecycle ─────────────────────────────────────────────

#[test]
fn test_gw_full_lifecycle_dirty_all_channels() {
    let (ss, mut bb) = make_bb();
    bb.submit_fact(&test_fact("mi_ground")).unwrap();
    bb.submit_intent(&test_intent("mi")).unwrap();
    let _ = ss.drain_kv_puts();

    bb.claim_intent("mi", "w").unwrap();
    bb.conclude_intent("mi", &serde_json::json!({})).unwrap();

    assert!(!ss.drain_kv_deletes().is_empty());
    assert!(!ss.drain_kv_puts().is_empty());
    assert!(!ss.drain_object_puts().is_empty());
}

// ─── Read state ─────────────────────────────────────────────────────────

#[test]
fn test_gw_read_state_from_session() {
    let (ss, mut bb) = make_bb();
    bb.submit_fact(&test_fact("pre")).unwrap();
    let s = Blackboard::read_state(&bb);
    assert_eq!(s.facts.len(), 1);
}

// ─── Edge: flush with concurrent session writes ────────────────────────

#[test]
fn test_gw_flush_after_multi_submit_dirty_channels() {
    let (ss, mut bb) = make_bb();
    for i in 0..5 {
        bb.submit_fact(&test_fact(&format!("mf{i}"))).unwrap();
    }
    let _ = ss.drain_kv_puts();

    let c = FlushCursor { last_flushed_at: String::new(), partition: "p1".into() };
    bb.flush_since(&c).unwrap();

    let blob = ss.drain_blob_puts();
    assert_eq!(blob.len(), 1, "one partition = one blob file");
    assert!(blob[0].0.contains("flush/facts"), "blob key should be flush path");
}

// ─── Edge: claim intent that was concluded in same session ──────────────

#[test]
fn test_gw_conclude_then_reclaim_same_intent_fails() {
    let (ss, mut bb) = make_bb();
    bb.submit_fact(&test_fact("rc_ground")).unwrap();
    bb.submit_intent(&test_intent("rc")).unwrap();
    bb.claim_intent("rc", "w1").unwrap();
    bb.conclude_intent("rc", &serde_json::json!({})).unwrap();
    let _ = ss.drain_kv_puts();
    let _ = ss.drain_kv_deletes();
    let _ = ss.drain_object_puts();

    // Intent no longer exists — claim should fail
    let r = bb.claim_intent("rc", "w2");
    assert!(r.is_err(), "concluded intent is gone — claim must fail");
}
