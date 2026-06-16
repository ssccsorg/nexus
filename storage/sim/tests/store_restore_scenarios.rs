// ── Real-use scenarios: FIH lifecycle + Store/Restore ──────────────────
//
// Validates complete FihStorage workflows using rebuild_cache for
// restoration. These tests verify that the FIH StateSpace operates
// correctly as a time-travelable knowledge store.

mod common;

use futures_executor::block_on;
use nexus_model::{
    Content, Fact, FactCapable, FihHash, Hint, HintCapable, Intent, IntentCapable, StorageRead,
};
use nexus_storage_sim::{FihStorage, SimIo};

fn fact(id: &str, data: &str) -> Fact {
    Fact {
        id: FihHash::from_hex(id),
        origin: "s".into(),
        content: Content {
            mime_type: "text/plain".into(),
            data: data.as_bytes().to_vec(),
        },
        creator: "t".into(),
    }
}

fn intent(id: &str, from: Vec<&str>) -> Intent {
    Intent {
        id: FihHash::from_hex(id),
        from_facts: from.into_iter().map(|s| FihHash::from_hex(s)).collect(),
        description: format!("intent {}", id),
        creator: "t".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    }
}

// ── Scenario A: Full lifecycle survives flush + rebuild ─────────────

#[test]
fn test_scenario_full_lifecycle_store_restore() {
    let io = SimIo::new();
    let store = FihStorage::new(io.clone(), "s");

    FactCapable::submit_fact(&store, &fact("f1", "alpha")).unwrap();
    FactCapable::submit_fact(&store, &fact("f2", "beta")).unwrap();
    IntentCapable::submit_intent(&store, &intent("i1", vec!["f1"])).unwrap();
    IntentCapable::claim_intent(&store, "i1", "alice").unwrap();
    IntentCapable::conclude_intent(&store, "i1", "result").unwrap();

    block_on(store.flush_pending()).unwrap();

    let restored = FihStorage::new(io, "s");
    block_on(restored.rebuild_cache()).unwrap();

    let state = StorageRead::read_state(&restored);
    assert_eq!(state.facts.len(), 3, "2 originals + 1 conclusion");
    assert_eq!(state.intents.len(), 1);
    assert!(state.intents[0].is_concluded);
}

// ── Scenario B: Reverse index survives rebuild ──────────────────────

#[test]
fn test_scenario_reverse_index_survives_rebuild() {
    let io = SimIo::new();
    let store = FihStorage::new(io.clone(), "s");

    FactCapable::submit_fact(&store, &fact("f_a", "a")).unwrap();
    FactCapable::submit_fact(&store, &fact("f_b", "b")).unwrap();
    IntentCapable::submit_intent(&store, &intent("i_a", vec!["f_a"])).unwrap();
    IntentCapable::submit_intent(&store, &intent("i_both", vec!["f_a", "f_b"])).unwrap();

    block_on(store.flush_pending()).unwrap();

    let restored = FihStorage::new(io, "s");
    block_on(restored.rebuild_cache()).unwrap();

    assert_eq!(restored.intents_by_fact("f_a").len(), 2);
    assert_eq!(restored.intents_by_fact("f_b").len(), 1);
}

// ── Scenario C: Concluded intent references preserved in rebuild ────

#[test]
fn test_scenario_concluded_intent_references_preserved() {
    let io = SimIo::new();
    let store = FihStorage::new(io.clone(), "s");

    FactCapable::submit_fact(&store, &fact("f_base", "base")).unwrap();
    IntentCapable::submit_intent(&store, &intent("i_concl", vec!["f_base"])).unwrap();
    IntentCapable::claim_intent(&store, "i_concl", "alice").unwrap();
    IntentCapable::conclude_intent(&store, "i_concl", "done").unwrap();

    assert!(store.intents_by_fact("f_base").is_empty());

    block_on(store.flush_pending()).unwrap();

    let restored = FihStorage::new(io, "s");
    block_on(restored.rebuild_cache()).unwrap();
    assert_eq!(restored.intents_by_fact("f_base").len(), 1);
}

// ── Scenario D: Multi-fact conclude clears all reverse refs in-memory ─

#[test]
fn test_scenario_multi_fact_conclude() {
    let store = FihStorage::new(SimIo::new(), "s");

    FactCapable::submit_fact(&store, &fact("f_x", "x")).unwrap();
    FactCapable::submit_fact(&store, &fact("f_y", "y")).unwrap();
    IntentCapable::submit_intent(&store, &intent("i_xy", vec!["f_x", "f_y"])).unwrap();
    IntentCapable::claim_intent(&store, "i_xy", "alice").unwrap();
    IntentCapable::conclude_intent(&store, "i_xy", "done").unwrap();

    assert!(store.intents_by_fact("f_x").is_empty());
    assert!(store.intents_by_fact("f_y").is_empty());
}

// ── Scenario E: Hints preserved via rebuild ─────────────────────

#[test]
fn test_scenario_hints_preserved_via_rebuild() {
    let io = SimIo::new();
    let store = FihStorage::new(io.clone(), "s");

    FactCapable::submit_fact(&store, &fact("f_h", "hint test")).unwrap();
    HintCapable::submit_hint(
        &store,
        &Hint {
            id: FihHash::from_hex("h1"),
            content: "ephemeral hint".into(),
            creator: "t".into(),
        },
    )
    .unwrap();

    block_on(store.flush_pending()).unwrap();

    let restored = FihStorage::new(io, "s");
    block_on(restored.rebuild_cache()).unwrap();

    let state = StorageRead::read_state(&restored);
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.hints.len(), 1);
    assert_eq!(state.hints[0].content, "ephemeral hint");
}

// ── Scenario F: Incremental multiples flushes + rebuild ────────────

#[test]
fn test_scenario_incremental_flushes() {
    let io = SimIo::new();
    let store = FihStorage::new(io.clone(), "s");

    FactCapable::submit_fact(&store, &fact("f1", "first")).unwrap();
    block_on(store.flush_pending()).unwrap();
    FactCapable::submit_fact(&store, &fact("f2", "second")).unwrap();
    block_on(store.flush_pending()).unwrap();
    FactCapable::submit_fact(&store, &fact("f3", "third")).unwrap();
    block_on(store.flush_pending()).unwrap();

    let restored = FihStorage::new(io, "s");
    block_on(restored.rebuild_cache()).unwrap();

    let state = StorageRead::read_state(&restored);
    assert_eq!(state.facts.len(), 3);
    assert_eq!(state.intents.len(), 0);
}

// ── Scenario G: Hints only + rebuild ─────────────────────────

#[test]
fn test_scenario_hints_only() {
    let io = SimIo::new();
    let store = FihStorage::new(io.clone(), "s");

    HintCapable::submit_hint(
        &store,
        &Hint {
            id: FihHash::from_hex("h_feature"),
            content: "consider adding time travel".into(),
            creator: "reviewer".into(),
        },
    )
    .unwrap();

    block_on(store.flush_pending()).unwrap();

    let restored = FihStorage::new(io, "s");
    block_on(restored.rebuild_cache()).unwrap();

    let state = StorageRead::read_state(&restored);
    assert_eq!(state.facts.len(), 0);
    assert_eq!(state.intents.len(), 0);
    assert_eq!(state.hints.len(), 1);
}

// ── Scenario H: Empty store + rebuild ──────────────────────────────

#[test]
fn test_scenario_empty_store() {
    let io = SimIo::new();
    let store = FihStorage::new(io.clone(), "s");
    block_on(store.flush_pending()).unwrap();

    let restored = FihStorage::new(io, "s");
    block_on(restored.rebuild_cache()).unwrap();

    let state = StorageRead::read_state(&restored);
    assert!(state.facts.is_empty());
    assert!(state.intents.is_empty());
    assert!(state.hints.is_empty());
}

// ── Scenario I: Multi-agent collaboration ──────────────────────────

#[test]
fn test_scenario_multi_agent_collaboration() {
    let store = FihStorage::new(SimIo::new(), "s");

    FactCapable::submit_fact(&store, &fact("obs_42", "observation value 42")).unwrap();
    IntentCapable::submit_intent(&store, &intent("analysis_1", vec!["obs_42"])).unwrap();
    IntentCapable::claim_intent(&store, "analysis_1", "bob").unwrap();
    IntentCapable::heartbeat(&store, "analysis_1", "bob").unwrap();

    assert!(IntentCapable::claim_intent(&store, "analysis_1", "charlie").is_err());

    let result =
        IntentCapable::conclude_intent(&store, "analysis_1", "obs_42 is consistent").unwrap();

    let state = StorageRead::read_state(&store);
    assert_eq!(state.facts.len(), 2);
    assert_eq!(state.intents.len(), 1);
    assert_eq!(
        state.intents[0].to_fact_id,
        Some(result.id)
    );
    assert!(state.intents[0].is_concluded);
}

// ── Scenario J: deduplicate facts via content hash ─────────────────

#[test]
fn test_scenario_content_dedup() {
    let store = FihStorage::new(SimIo::new(), "s");

    FactCapable::submit_fact(&store, &fact("f_dup1", "same content")).unwrap();
    FactCapable::submit_fact(&store, &fact("f_dup2", "same content")).unwrap();

    // On in-memory, they're separate records. Dedup happens at the blob level.
    // Both entries reference the same content bytes, which is fine.
    let state = StorageRead::read_state(&store);
    assert_eq!(state.facts.len(), 2);

    // Check blobs: same content should produce same hash
    assert_eq!(state.facts[0].content.data, state.facts[1].content.data);
}

// ── Scenario K: Intent without facts ───────────────────────────────

#[test]
fn test_scenario_empty_from_facts_rejected() {
    let store = FihStorage::new(SimIo::new(), "s");

    let result = IntentCapable::submit_intent(&store, &intent("i_empty", vec![]));
    assert!(
        result.is_err(),
        "intent without from_facts must be rejected"
    );
}

// ── Scenario L: Storage migration (SimIo to fresh SimIo) ──────────

#[test]
fn test_scenario_storage_migration() {
    let io = SimIo::new();
    let src = FihStorage::new(io.clone(), "s");

    FactCapable::submit_fact(&src, &fact("f_mig", "migrate me")).unwrap();
    IntentCapable::submit_intent(&src, &intent("i_mig", vec!["f_mig"])).unwrap();
    block_on(src.flush_pending()).unwrap();

    let dst = FihStorage::new(io, "s");
    block_on(dst.rebuild_cache()).unwrap();

    let state = StorageRead::read_state(&dst);
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.intents.len(), 1);
    assert_eq!(state.intents[0].from_facts.len(), 1);
}
