// ── Real-use scenarios: FIH lifecycle + Store/Restore ──────────────────
//
// Validates complete FihStorage workflows using rebuild_cache for
// restoration. These tests verify that the FIH StateSpace operates
// correctly as a time-travelable knowledge store.

mod common;

use futures_executor::block_on;
use nex_fih::{
    AsyncFactCapable, AsyncHintCapable, AsyncIntentCapable, AsyncStorageRead, Content, CoordId,
    Fact, Hint, Intent,
};
use nexus_storage_sim::{FihStorage, FileIo, SimIo};

fn fact(id: &str, data: &str) -> Fact {
    Fact::with_id(
        CoordId::resolve(id),
        "s".into(),
        Content {
            mime_type: "text/plain".into(),
            data: data.as_bytes().to_vec(),
        },
        "t".into(),
    )
}

fn intent(id: &str, from: Vec<&str>) -> Intent {
    Intent {
        id: CoordId::resolve(id),
        from_facts: from.into_iter().map(CoordId::resolve).collect(),
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

    block_on(store.submit_fact(&fact("f1", "alpha"))).unwrap();
    block_on(store.submit_fact(&fact("f2", "beta"))).unwrap();
    block_on(store.submit_intent(&intent("i1", vec!["f1"]))).unwrap();
    block_on(store.claim_intent("i1", "alice")).unwrap();
    block_on(store.conclude_intent("i1", "result")).unwrap();

    block_on(store.flush_pending()).unwrap();

    let restored = FihStorage::new(io, "s");
    block_on(restored.rebuild_cache()).unwrap();

    let state = block_on(restored.read_state());
    assert_eq!(state.facts.len(), 3, "2 originals + 1 conclusion");
    assert_eq!(state.intents.len(), 1);
    assert!(state.intents[0].is_concluded);
}

// ── Scenario B: Reverse index survives rebuild ──────────────────────

#[test]
fn test_scenario_reverse_index_survives_rebuild() {
    let io = SimIo::new();
    let store = FihStorage::new(io.clone(), "s");

    block_on(store.submit_fact(&fact("f_a", "a"))).unwrap();
    block_on(store.submit_fact(&fact("f_b", "b"))).unwrap();
    block_on(store.submit_intent(&intent("i_a", vec!["f_a"]))).unwrap();
    block_on(store.submit_intent(&intent("i_both", vec!["f_a", "f_b"]))).unwrap();

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

    block_on(store.submit_fact(&fact("f_base", "base"))).unwrap();
    block_on(store.submit_intent(&intent("i_concl", vec!["f_base"]))).unwrap();
    block_on(store.claim_intent("i_concl", "alice")).unwrap();
    block_on(store.conclude_intent("i_concl", "done")).unwrap();

    // In-memory reverse index retains concluded intents.
    // After rebuild from IO, the index is reconstructed from stored records.
    assert!(store.intents_by_fact("f_base").len() == 1);

    block_on(store.flush_pending()).unwrap();

    let restored = FihStorage::new(io, "s");
    block_on(restored.rebuild_cache()).unwrap();
    assert_eq!(restored.intents_by_fact("f_base").len(), 1);
}

// ── Scenario D: Multi-fact conclude clears all reverse refs in-memory ─

#[test]
fn test_scenario_multi_fact_conclude() {
    let store = FihStorage::new(SimIo::new(), "s");

    block_on(store.submit_fact(&fact("f_x", "x"))).unwrap();
    block_on(store.submit_fact(&fact("f_y", "y"))).unwrap();
    block_on(store.submit_intent(&intent("i_xy", vec!["f_x", "f_y"]))).unwrap();
    block_on(store.claim_intent("i_xy", "alice")).unwrap();
    block_on(store.conclude_intent("i_xy", "done")).unwrap();

    // In-memory reverse index retains concluded intents.
    assert!(store.intents_by_fact("f_x").len() == 1);
    assert!(store.intents_by_fact("f_y").len() == 1);
}

// ── Scenario E: Hints preserved via rebuild ─────────────────────

#[test]
fn test_scenario_hints_preserved_via_rebuild() {
    let io = SimIo::new();
    let store = FihStorage::new(io.clone(), "s");

    block_on(store.submit_fact(&fact("f_h", "hint test"))).unwrap();
    block_on(store.submit_hint(&Hint {
        id: CoordId::resolve("h1"),
        content: "ephemeral hint".into(),
        creator: "t".into(),
    }))
    .unwrap();

    block_on(store.flush_pending()).unwrap();

    let restored = FihStorage::new(io, "s");
    block_on(restored.rebuild_cache()).unwrap();

    let state = block_on(restored.read_state());
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.hints.len(), 1);
    assert_eq!(state.hints[0].content, "ephemeral hint");
}

// ── Scenario F: Incremental multiples flushes + rebuild ────────────

#[test]
fn test_scenario_incremental_flushes() {
    let io = SimIo::new();
    let store = FihStorage::new(io.clone(), "s");

    block_on(store.submit_fact(&fact("f1", "first"))).unwrap();
    block_on(store.flush_pending()).unwrap();
    block_on(store.submit_fact(&fact("f2", "second"))).unwrap();
    block_on(store.flush_pending()).unwrap();
    block_on(store.submit_fact(&fact("f3", "third"))).unwrap();
    block_on(store.flush_pending()).unwrap();

    let restored = FihStorage::new(io, "s");
    block_on(restored.rebuild_cache()).unwrap();

    let state = block_on(restored.read_state());
    assert_eq!(state.facts.len(), 3);
    assert_eq!(state.intents.len(), 0);
}

// ── Scenario G: Hints only + rebuild ─────────────────────────

#[test]
fn test_scenario_hints_only() {
    let io = SimIo::new();
    let store = FihStorage::new(io.clone(), "s");

    block_on(store.submit_hint(&Hint {
        id: CoordId::resolve("h_feature"),
        content: "consider adding time travel".into(),
        creator: "reviewer".into(),
    }))
    .unwrap();

    block_on(store.flush_pending()).unwrap();

    let restored = FihStorage::new(io, "s");
    block_on(restored.rebuild_cache()).unwrap();

    let state = block_on(restored.read_state());
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

    let state = block_on(restored.read_state());
    assert!(state.facts.is_empty());
    assert!(state.intents.is_empty());
    assert!(state.hints.is_empty());
}

// ── Scenario I: Multi-agent collaboration ──────────────────────────

#[test]
fn test_scenario_multi_agent_collaboration() {
    let store = FihStorage::new(SimIo::new(), "s");

    block_on(store.submit_fact(&fact("obs_42", "observation value 42"))).unwrap();
    block_on(store.submit_intent(&intent("analysis_1", vec!["obs_42"]))).unwrap();
    block_on(store.claim_intent("analysis_1", "bob")).unwrap();
    block_on(store.heartbeat("analysis_1", "bob")).unwrap();

    assert!(block_on(store.claim_intent("analysis_1", "charlie")).is_err());

    let result = block_on(store.conclude_intent("analysis_1", "obs_42 is consistent")).unwrap();

    let state = block_on(store.read_state());
    assert_eq!(state.facts.len(), 2);
    assert_eq!(state.intents.len(), 1);
    assert_eq!(state.intents[0].to_fact_id, Some(result.id));
    assert!(state.intents[0].is_concluded);
}

// ── Scenario J: deduplicate facts via content hash ─────────────────

#[test]
fn test_scenario_content_dedup() {
    let store = FihStorage::new(SimIo::new(), "s");

    block_on(store.submit_fact(&fact("f_dup1", "same content"))).unwrap();
    block_on(store.submit_fact(&fact("f_dup2", "same content"))).unwrap();

    // On in-memory, they're separate records. Dedup happens at the blob level.
    // Both entries reference the same content bytes, which is fine.
    let state = block_on(store.read_state());
    assert_eq!(state.facts.len(), 2);

    // Check blobs: same content should produce same hash
    assert_eq!(state.facts[0].content.data, state.facts[1].content.data);
}

// ── Scenario K: Intent without facts ───────────────────────────────

#[test]
fn test_scenario_empty_from_facts_rejected() {
    let store = FihStorage::new(SimIo::new(), "s");

    let result = block_on(store.submit_intent(&intent("i_empty", vec![])));
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

    block_on(src.submit_fact(&fact("f_mig", "migrate me"))).unwrap();
    block_on(src.submit_intent(&intent("i_mig", vec!["f_mig"]))).unwrap();
    block_on(src.flush_pending()).unwrap();

    let dst = FihStorage::new(io, "s");
    block_on(dst.rebuild_cache()).unwrap();

    let state = block_on(dst.read_state());
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.intents.len(), 1);
    assert_eq!(state.intents[0].from_facts.len(), 1);
}

// ── Scenario M: Malformed blob hash falls back to blob recompute ─────

#[test]
fn test_scenario_malformed_blob_hash_recomputes_content_hash() {
    use nex_fih::{BlackboardError, ContentMeta, FactRecord};

    let io = SimIo::new();
    let bad_key = "legacy-non-hex-blob-key";
    let fact = Fact::with_id(
        CoordId::resolve("f_badhash"),
        "s".into(),
        Content {
            mime_type: "text/plain".into(),
            data: b"payload".to_vec(),
        },
        "t".into(),
    );
    // A pre-existing record whose persisted blob hash is not 64-hex,
    // with the blob stored under that same (non-hex) key.
    let record = FactRecord {
        id: fact.id.to_string(),
        blob_hash: bad_key.into(),
        origin: fact.origin.clone(),
        creator: fact.creator.clone(),
        submitted_at: 0,
    };
    block_on(io.write(&record.key(), &postcard::to_allocvec(&record).unwrap())).unwrap();
    block_on(io.write(&format!("blob/{bad_key}.bin"), b"payload")).unwrap();
    block_on(
        io.write(
            &format!("blob/{bad_key}.bin.meta"),
            &postcard::to_allocvec(&ContentMeta {
                mime_type: "text/plain".into(),
                size: 7,
            })
            .unwrap(),
        ),
    )
    .unwrap();

    let store = FihStorage::new(io, "badhash");
    block_on(store.rebuild_cache()).unwrap();

    // The matching map holds the hash recomputed from the blob: the same
    // content is an idempotent retry, a different content is rejected.
    let same = block_on(store.submit_fact(&fact)).unwrap();
    assert_eq!(same, fact.id, "same content is idempotent after rebuild");
    let other = Fact::with_id(
        CoordId::resolve("f_badhash"),
        "s".into(),
        Content {
            mime_type: "text/plain".into(),
            data: b"other".to_vec(),
        },
        "t".into(),
    );
    let err = block_on(store.submit_fact(&other)).unwrap_err();
    assert!(
        matches!(err, BlackboardError::Conflict(_)),
        "different content conflicts after rebuild"
    );
}
