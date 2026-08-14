// ── Time Machine tests ──────────────────────────────────────────────────
//
// Validates FIH StateSpace as a 4D time-travelable storage:
//   1. Storage migration (SimIo → fresh FihStorage)
//   2. Time-travel consistency (as_of window excludes future)
//   3. Content deduplication (same blob stored once)
//   4. Full StateSpace round-trip (submit → flush → rebuild → verify)
//   5. Eviction durability (records deleted from io)

mod common;

use futures_executor::block_on;
use nex_fih::{
    AsyncEvictCapable, AsyncFactCapable, AsyncFilterCapable, AsyncHintCapable, AsyncIntentCapable,
    AsyncStorageRead, Content, CoordId, Fact, Hint, Intent, StateFilter,
};
use nexus_storage_sim::{EntityStore, FihStorage, SimIo, SyncFileIo};

// ── Helpers ──────────────────────────────────────────────────────────────

fn store() -> FihStorage<SimIo> {
    FihStorage::new(SimIo::new(), "tm")
}

fn submit_fact(store: &FihStorage<SimIo>, id: &str, data: &str) {
    block_on(store.submit_fact(&Fact::with_id(
        CoordId::resolve(id),
        "tm".into(),
        Content {
            mime_type: "text/plain".into(),
            data: data.as_bytes().to_vec(),
        },
        "tester".into(),
    )))
    .unwrap();
}

fn submit_intent(store: &FihStorage<SimIo>, id: &str, from: &[&str]) {
    block_on(store.submit_intent(&Intent {
        id: CoordId::resolve(id),
        from_facts: from.iter().map(|s| CoordId::resolve(s)).collect(),
        description: format!("intent {}", id),
        creator: "tester".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    }))
    .unwrap();
}

// ── Test 2: Storage migration (SimIo → fresh FihStorage) ────────
//
// Simulate moving FIH data between stores: flush everything into io_a,
// then a new store on the same io instance reads it all back.

#[test]
fn test_storage_migration() {
    let io = SimIo::new();

    // Source store
    let src = FihStorage::new(io.clone(), "tm");
    submit_fact(&src, "f1", "data1");
    submit_fact(&src, "f2", "data2");
    submit_intent(&src, "i1", &["f1", "f2"]);
    block_on(src.flush_pending()).unwrap();

    // Destination store — reads from same io
    let dst = FihStorage::new(io, "tm");
    block_on(dst.rebuild_cache()).unwrap();

    let state = block_on(dst.read_state());
    assert_eq!(state.facts.len(), 2);
    assert_eq!(state.intents.len(), 1);
    assert_eq!(state.intents[0].from_facts.len(), 2);
}

// ── Test 3: Time-travel consistency ─────────────────────────────────────
//
// Submit Fact at t1, then Intent at t2 referencing that Fact.
// as_of(t=midpoint) must show the Fact but NOT the Intent.

#[test]
fn test_time_travel_consistency() {
    // FakeClock: start at 1_000_000_000, step 1_000_000_000 each call
    let clock = common::FakeClock::with_step(1_000_000_000, 1_000_000_000);
    let store = FihStorage::with_clock(SimIo::new(), "tm", Box::new(clock));

    // Fact submitted at clock call 1 (1_000_000_000), TimeIndex at call 2 (2_000_000_000)
    submit_fact(&store, "f_pre", "pre");
    // Intent submitted after (later clock values)
    submit_intent(&store, "i_post", &["f_pre"]);

    // Full state has both
    let full = block_on(store.read_state());
    assert_eq!(full.facts.len(), 1);
    assert_eq!(full.intents.len(), 1);

    // Time-travel to t=1_500_000_000: Fact (submitted_at=1G) included,
    // Intent (created_at=2G) excluded because 2G > 1.5G.
    let past = block_on(store.read_state_filtered(&StateFilter {
        axis_hints: None,
        until: Some("1500000000".to_string()),
        ..Default::default()
    }));
    assert_eq!(past.facts.len(), 1, "fact submitted before midpoint");
    assert_eq!(
        past.intents.len(),
        0,
        "intent submitted after midpoint must be excluded"
    );
}

// ── Test 4: Content deduplication ──────────────────────────────────────
//
// Submit same content with a different fact ID. Blob must be stored once.

#[test]
fn test_content_dedup() {
    let io = SimIo::new();
    let store = FihStorage::new(io.clone(), "tm");

    submit_fact(&store, "f_dup_a", "shared content");
    submit_fact(&store, "f_dup_b", "shared content");
    block_on(store.flush_pending()).unwrap();

    let blob_keys = SyncFileIo::new(io).list("blob/").unwrap();
    // "shared content" hash → exactly 1 blob entry (not 2)
    let bin_count = blob_keys.iter().filter(|k| k.ends_with(".bin")).count();
    assert_eq!(bin_count, 1, "same content stored once via content hash");
}

// ── Test 5: Full StateSpace round-trip ──────────────────────────────────
//
// Submit facts + intents + hints, conclude, then read_state must match
// the expected BoardState exactly.

#[test]
fn test_full_statespace_round_trip() {
    let store = store();

    submit_fact(&store, "f1", "one");
    submit_fact(&store, "f2", "two");
    submit_intent(&store, "i1", &["f1"]);
    block_on(store.submit_hint(&Hint {
        id: CoordId::resolve("h1"),
        content: "hint one".into(),
        creator: "tester".into(),
    }))
    .unwrap();

    block_on(store.claim_intent("i1", "alice")).unwrap();
    block_on(store.heartbeat("i1", "alice")).unwrap();
    block_on(store.conclude_intent("i1", "result one")).unwrap();

    let state = block_on(store.read_state());
    assert_eq!(state.facts.len(), 3, "2 original + 1 conclusion");
    assert_eq!(state.intents.len(), 1);
    assert_eq!(state.hints.len(), 1);
    assert!(state.intents[0].is_concluded);
    assert!(state.intents[0].concluded_at.is_some());
    assert!(state.intents[0].concluded_at.unwrap() > 0);
}

// ── Test 7: Empty StateSpace is valid ───────────────────────────────────

#[test]
fn test_empty_statespace_is_valid() {
    let io = SimIo::new();
    let store = FihStorage::new(io.clone(), "tm");
    block_on(store.flush_pending()).unwrap();

    let state = block_on(store.read_state());
    assert!(state.facts.is_empty());
    assert!(state.intents.is_empty());
    assert!(state.hints.is_empty());
}

// ── Test 8: Eviction preserves Fact, removes old Hint ───────────────────

#[test]
fn test_eviction_preserves_fact_removes_old_hint() {
    let store = store();
    submit_fact(&store, "f_keep", "keep me");
    block_on(store.submit_hint(&Hint {
        id: CoordId::resolve("h_old"),
        content: "old hint".into(),
        creator: "tester".into(),
    }))
    .unwrap();

    block_on(store.evict_before("99999999999")).unwrap();

    // Eviction deletes the record files from io, so the state read and
    // the in-memory store agree: the hint is gone from both.
    assert_eq!(
        block_on(store.hint_store.len()),
        0,
        "old hint must be evicted from memory"
    );
    let state = block_on(store.read_state());
    assert_eq!(state.hints.len(), 0, "old hint must be evicted from io");
    assert_eq!(state.facts.len(), 1, "fact must survive eviction");
}
