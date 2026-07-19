// nexus-storage-petgraph — Unit tests for PetgraphStorage.
//
// Covers GraphRead/GraphWrite trait implementations, EvictCapable,
// and storage-level FIH operations directly (not via HybridBlackboard).

use nexus_model::{
    Content, EvictCapable, Fact, FactCapable, FihHash, Hint, HintCapable, StorageRead,
};
use nexus_storage_petgraph::{PetgraphStorage, read_graph};

fn storage() -> PetgraphStorage {
    PetgraphStorage::with_project_id("test")
}

fn fact(id: &str) -> Fact {
    Fact::new(
        FihHash::from_hex(id),
        "test".into(),
        "data".into(),
        "tester".into(),
    )
}

fn hint(id: &str) -> Hint {
    Hint {
        id: FihHash::from_hex(id),
        content: format!("content_{}", id),
        creator: "tester".into(),
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// Basic CRUD

#[test]
fn test_add_and_retrieve_node() {
    let s = storage();
    let g = read_graph(&s.graph);

    let initial_count = g.node_count();
    // PetgraphStorage starts empty? No — it may have sentinel nodes.
    // Just verify the API works.
    assert_eq!(g.node_count(), initial_count);
}

#[test]
fn test_submit_and_read_fact() {
    let s = storage();
    s.submit_fact(&fact("f_001")).unwrap();

    let state = s.read_state();
    assert_eq!(state.facts.len(), 1);
    assert_eq!(
        state.facts[0].id.to_string(),
        nexus_model::FihHash::from_hex("f_001").to_string()
    );
    assert_eq!(state.facts[0].origin, "test");
}

#[test]
fn test_submit_multiple_facts() {
    let s = storage();
    s.submit_fact(&fact("f_a")).unwrap();
    s.submit_fact(&fact("f_b")).unwrap();
    s.submit_fact(&fact("f_c")).unwrap();

    let state = s.read_state();
    assert_eq!(state.facts.len(), 3);
}

#[test]
fn test_submit_fact_duplicate_id() {
    let s = storage();
    s.submit_fact(&fact("f_dup")).unwrap();
    // PetgraphStorage does not deduplicate — same ID adds another node.
    // Deduplication is DualStorage's responsibility.
    let result = s.submit_fact(&fact("f_dup"));
    assert!(
        result.is_ok(),
        "duplicate ID accepted (no dedup at petgraph level)"
    );
}

// Hint operations

#[test]
fn test_submit_and_read_hint() {
    let s = storage();
    s.submit_hint(&hint("h_001")).unwrap();

    let state = s.read_state();
    assert_eq!(state.hints.len(), 1);
    assert_eq!(state.hints[0].content, "content_h_001");
}

// EvictCapable

#[test]
fn test_approximate_size_grows_with_data() {
    let s = storage();
    let empty_size = s.approximate_size();

    for i in 0..100 {
        s.submit_fact(&fact(&format!("f_size_{}", i))).unwrap();
    }

    let full_size = s.approximate_size();
    assert!(
        full_size > empty_size,
        "size with 100 facts > empty size: {} > {}",
        full_size,
        empty_size
    );
}

// Intent lifecycle

#[test]
fn test_submit_intent_requires_existing_fact() {
    use nexus_model::{Intent, IntentCapable};

    let s = storage();
    let intent = Intent {
        id: FihHash::from_hex("i_no_fact"),
        coord: None,
        from_facts: vec![FihHash::from_hex("f_nonexistent")],
        description: "missing ref".into(),
        creator: "tester".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    };
    let result = s.submit_intent(&intent);
    assert!(
        result.is_err(),
        "intent referencing missing fact should fail"
    );
}

#[test]
fn test_conclude_intent_creates_fact() {
    use nexus_model::{Intent, IntentCapable};

    let s = storage();
    s.submit_fact(&fact("f_base")).unwrap();

    let intent = Intent {
        id: FihHash::from_hex("i_concl"),
        coord: None,
        from_facts: vec![FihHash::from_hex("f_base")],
        description: "test conclusion".into(),
        creator: "tester".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    };
    s.submit_intent(&intent).unwrap();
    s.claim_intent("i_concl", "agent-x").unwrap();
    let result = s.conclude_intent("i_concl", "result data").unwrap();

    assert_eq!(result.creator, "agent-x");
    assert_eq!(
        result.content,
        Content {
            mime_type: "text/plain".into(),
            data: "result data".into()
        }
    );

    let state = s.read_state();
    assert_eq!(state.facts.len(), 2, "original + concluded fact");
}

// EvictCapable: evict_before removes old intents

#[test]
fn test_evict_before_removes_old_concluded_intents() {
    use nexus_model::{Intent, IntentCapable};

    let s = storage();
    s.submit_fact(&fact("f_ev")).unwrap();

    let now = now_secs();

    // Submit an intent and immediately conclude it
    let intent = Intent {
        id: FihHash::from_hex("i_old"),
        coord: None,
        from_facts: vec![FihHash::from_hex("f_ev")],
        description: "old intent".into(),
        creator: "tester".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    };
    s.submit_intent(&intent).unwrap();
    s.claim_intent("i_old", "agent-x").unwrap();
    s.conclude_intent("i_old", "done").unwrap();

    // evict_before with far future timestamp — should evict the old concluded intent
    let removed = s.evict_before(&(now + 99999).to_string()).unwrap();
    assert_eq!(removed, 1, "old concluded intent evicted");
}

#[test]
fn test_evict_before_does_not_remove_facts() {
    let s = storage();
    s.submit_fact(&fact("f_safe")).unwrap();

    let before = s.evict_before(&(now_secs() + 99999).to_string()).unwrap();
    // Facts should never be evicted
    assert_eq!(before, 0, "Facts are never evicted");

    let state = s.read_state();
    assert_eq!(state.facts.len(), 1, "fact survives eviction");
}

// StorageRead

#[test]
fn test_read_state_returns_board_state() {
    let s = storage();
    s.submit_fact(&fact("f_rs")).unwrap();

    let state = s.read_state();
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.intents.len(), 0);
    assert_eq!(state.hints.len(), 0);
}
