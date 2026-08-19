// ── Unified 12-axis traversal filter tests ─────────────────────────────
//
// read_state_filtered now iterates the single 12-axis coordinate store
// instead of the per-type fast-path indexes. These tests lock in the
// behaviors the unified traversal provides uniformly:
//
//   - creator filters return intents and hints alongside facts (the old
//     fast paths returned empty intent and hint lists)
//   - offset and limit apply on creator-filtered facts (the old fast
//     paths ignored offset)
//   - fact content is materialized from the in-memory record, so reads
//     see full data without a pending blob
//   - intent time filters apply over the unified traversal
//   - hint_ids filtering matches on identity coordinates

mod common;

use futures_executor::block_on;
use nex_fih::{
    AsyncFactCapable, AsyncFilterCapable, AsyncHintCapable, AsyncIntentCapable, Content, CoordId,
    Fact, Hint, StateFilter,
};
use nexus_storage_sim::{FihStorage, SimIo};

use crate::common::{fact, intent};

// ── Test 1: creator filter returns intents and hints too ────────────────

#[test]
fn test_creator_filter_returns_intents_and_hints() {
    let store = FihStorage::new(SimIo::new(), "unified_creator");
    block_on(store.submit_fact(&fact("f1"))).unwrap();
    block_on(store.submit_intent(&intent("i1", vec!["f1"]))).unwrap();
    block_on(store.submit_hint(&Hint {
        id: CoordId::resolve("h1"),
        content: "hint body".into(),
        creator: "t".into(),
    }))
    .unwrap();
    // A hint from another creator must be excluded by the creator filter.
    block_on(store.submit_hint(&Hint {
        id: CoordId::resolve("h_other"),
        content: "other hint".into(),
        creator: "other".into(),
    }))
    .unwrap();

    let state = block_on(store.read_state_filtered(&StateFilter {
        creator: Some("t".into()),
        ..Default::default()
    }));
    assert_eq!(state.facts.len(), 1, "creator filter keeps the fact");
    assert_eq!(
        state.intents.len(),
        1,
        "unified traversal returns intents for creator filters"
    );
    assert_eq!(
        state.hints.len(),
        1,
        "creator filter excludes hints from other creators"
    );
    assert_eq!(
        state.hints[0].id.to_string(),
        CoordId::resolve("h1").to_string(),
        "only the matching creator's hint is returned"
    );
}

// ── Test 2: offset and limit apply on creator-filtered facts ───────────

#[test]
fn test_offset_and_limit_on_creator_filter() {
    let store = FihStorage::new(SimIo::new(), "unified_offset");
    for i in 0..4 {
        let mut f = fact(&format!("f{i}"));
        f.creator = "alice".into();
        block_on(store.submit_fact(&f)).unwrap();
    }

    let paged = block_on(store.read_state_filtered(&StateFilter {
        creator: Some("alice".into()),
        offset: Some(1),
        limit: Some(2),
        ..Default::default()
    }));
    assert_eq!(paged.facts.len(), 2, "offset+limit page is 2 facts");

    let offset_only = block_on(store.read_state_filtered(&StateFilter {
        creator: Some("alice".into()),
        offset: Some(2),
        ..Default::default()
    }));
    assert_eq!(offset_only.facts.len(), 2, "offset-only drops 2 facts");
}

// ── Test 3: fact content materialized from the in-memory record ────────

#[test]
fn test_fact_content_materialized_from_record() {
    let store = FihStorage::new(SimIo::new(), "unified_content");
    let f = Fact::with_id(
        CoordId::resolve("f_body"),
        "t".into(),
        Content {
            mime_type: "text/plain".into(),
            data: b"payload bytes".to_vec(),
        },
        "t".into(),
    );
    block_on(store.submit_fact(&f)).unwrap();

    let state = block_on(store.read_state_filtered(&StateFilter {
        creator: Some("t".into()),
        ..Default::default()
    }));
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.facts[0].content.data, b"payload bytes".to_vec());
    assert_eq!(state.facts[0].content.mime_type, "text/plain");
}

// ── Test 4: intent since filter over the unified traversal ─────────────

#[test]
fn test_intent_since_filter_over_unified_store() {
    let store = FihStorage::with_clock(
        SimIo::new(),
        "unified_intent_since",
        Box::new(common::FakeClock::with_step(1_000_000_000, 1_000_000_000)),
    );
    block_on(store.submit_fact(&fact("f1"))).unwrap();
    // Clock now_secs reads 2 after the fact submission advanced now_nanos.
    block_on(store.submit_intent(&intent("i1", vec!["f1"]))).unwrap();
    block_on(store.submit_intent(&intent("i2", vec!["f1"]))).unwrap();

    let before = block_on(store.read_state_filtered(&StateFilter {
        since: Some("1000000000".into()),
        ..Default::default()
    }));
    assert_eq!(
        before.intents.len(),
        2,
        "intents created at 2s pass since=1s"
    );

    let after = block_on(store.read_state_filtered(&StateFilter {
        since: Some("3000000000".into()),
        ..Default::default()
    }));
    assert_eq!(after.intents.len(), 0, "intents at 2s fail since=3s");
}

// ── Test 5: hint_ids filter matches identity coordinates ───────────────

#[test]
fn test_hint_ids_filter_over_unified_store() {
    let store = FihStorage::new(SimIo::new(), "unified_hint_ids");
    for i in 0..3 {
        block_on(store.submit_hint(&Hint {
            id: CoordId::resolve(&format!("h{i}")),
            content: format!("hint {i}"),
            creator: "t".into(),
        }))
        .unwrap();
    }

    let state = block_on(store.read_state_filtered(&StateFilter {
        hint_ids: Some(vec!["h1".into()]),
        ..Default::default()
    }));
    assert_eq!(state.hints.len(), 1);
    assert_eq!(
        state.hints[0].id.to_string(),
        CoordId::resolve("h1").to_string()
    );
}

// ── Test 6: intent description materialized from io after a flush ─────

#[test]
fn test_intent_description_materialized_after_flush() {
    let io = SimIo::new();
    let store = FihStorage::new(io.clone(), "unified_desc");
    block_on(store.submit_fact(&fact("f1"))).unwrap();
    let mut i = intent("i1", vec!["f1"]);
    i.description = "persisted description".into();
    block_on(store.submit_intent(&i)).unwrap();
    block_on(store.flush_pending()).unwrap();

    // After the flush the description blob lives on io, not in pending;
    // the filtered read must still materialize it via the async boundary.
    let state = block_on(store.read_state_filtered(&StateFilter {
        ..Default::default()
    }));
    assert_eq!(state.intents.len(), 1);
    assert_eq!(state.intents[0].description, "persisted description");
}
