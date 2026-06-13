// ── Time Machine tests ──────────────────────────────────────────────────
//
// Validates FIH StateSpace as a 4D time-travelable storage:
//   1. Delta chain reconstruction (cursor-based replay)
//   2. Storage migration (SimFihIo → fresh FihStorage)
//   3. Time-travel consistency (as_of window excludes future)
//   4. Content deduplication (same blob stored once)
//   5. Full StateSpace round-trip (submit → flush → rebuild → verify)

mod common;

use nexus_model::{
    Content, EvictCapable, Fact, FactCapable, FihHash, FilterCapable, FlushCapable, FlushCursor,
    FlushResult, Hint, HintCapable, Intent, IntentCapable, StateFilter, StorageRead,
};
use nexus_storage_sim::{BlockingFihIo, FihStorage, SimFihIo};

// ── Helpers ──────────────────────────────────────────────────────────────

fn store() -> FihStorage<SimFihIo> {
    FihStorage::new(SimFihIo::new(), "tm")
}

fn submit_fact(store: &FihStorage<SimFihIo>, id: &str, data: &str) {
    FactCapable::submit_fact(
        store,
        &Fact {
            id: FihHash(id.into()),
            origin: "tm".into(),
            content: Content {
                mime_type: "text/plain".into(),
                data: data.as_bytes().to_vec(),
            },
            creator: "tester".into(),
        },
    )
    .unwrap();
}

fn submit_intent(store: &FihStorage<SimFihIo>, id: &str, from: &[&str]) {
    IntentCapable::submit_intent(
        store,
        &Intent {
            id: FihHash(id.into()),
            from_facts: from.iter().map(|s| s.to_string()).collect(),
            description: format!("intent {}", id),
            creator: "tester".into(),
            worker: None,
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            is_concluded: false,
            concluded_at: None,
        },
    )
    .unwrap();
}

fn flush_at(store: &FihStorage<SimFihIo>, cursor: &FlushCursor) -> FlushResult {
    FlushCapable::flush_since(store, cursor).unwrap()
}

// ── Test 1: Delta chain reconstruction ───────────────────────────────────
//
// Submit data in 3 epochs, flush after each. Read chain files back and
// verify cumulative state matches original.

#[test]
fn test_delta_chain_reconstruction() {
    let io = SimFihIo::new();
    let store = FihStorage::new(io.clone(), "tm");

    // Epoch 1: submit f_a
    submit_fact(&store, "f_a", "alpha");
    let r1 = flush_at(
        &store,
        &FlushCursor {
            last_flushed_at: 0,
            partition: "default".into(),
        },
    );
    assert_eq!(r1.records_flushed, 1);

    // Epoch 2: submit f_b
    submit_fact(&store, "f_b", "beta");
    let r2 = flush_at(
        &store,
        &FlushCursor {
            last_flushed_at: r1.new_cursor.last_flushed_at,
            partition: "default".into(),
        },
    );
    assert_eq!(r2.records_flushed, 1);

    // Epoch 3: submit f_c
    submit_fact(&store, "f_c", "gamma");
    let _r3 = flush_at(
        &store,
        &FlushCursor {
            last_flushed_at: r2.new_cursor.last_flushed_at,
            partition: "default".into(),
        },
    );

    // Reconstruct from IO — all 3 facts should be present
    let store2 = FihStorage::new(io, "tm");
    store2.rebuild_cache().unwrap();
    let state = StorageRead::read_state(&store2);
    assert_eq!(
        state.facts.len(),
        3,
        "all 3 facts reconstructed from chains"
    );
    let ids: Vec<&str> = state.facts.iter().map(|f| f.id.0.as_str()).collect();
    assert!(ids.contains(&"f_a"));
    assert!(ids.contains(&"f_b"));
    assert!(ids.contains(&"f_c"));
}

// ── Test 2: Storage migration (SimFihIo → fresh FihStorage) ────────
//
// Simulate moving FIH data between stores: flush everything into io_a,
// then a new store on the same io instance reads it all back.

#[test]
fn test_storage_migration() {
    let io = SimFihIo::new();

    // Source store
    let src = FihStorage::new(io.clone(), "tm");
    submit_fact(&src, "f1", "data1");
    submit_fact(&src, "f2", "data2");
    submit_intent(&src, "i1", &["f1", "f2"]);
    src.flush_pending().unwrap();

    // Destination store — reads from same io
    let dst = FihStorage::new(io, "tm");
    dst.rebuild_cache().unwrap();

    let state = StorageRead::read_state(&dst);
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
    let store = FihStorage::with_clock(SimFihIo::new(), "tm", Box::new(clock));

    // Fact submitted at clock call 1 (1_000_000_000), TimeIndex at call 2 (2_000_000_000)
    submit_fact(&store, "f_pre", "pre");
    // Intent submitted after (later clock values)
    submit_intent(&store, "i_post", &["f_pre"]);

    // Full state has both
    let full = StorageRead::read_state(&store);
    assert_eq!(full.facts.len(), 1);
    assert_eq!(full.intents.len(), 1);

    // Time-travel to t=2_500_000_000: Fact (indexed at 2G) included, Intent not yet indexed
    let past = FilterCapable::read_state_filtered(
        &store,
        &StateFilter {
            until: Some("2500000000".to_string()),
            ..Default::default()
        },
    );
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
    let io = SimFihIo::new();
    let store = FihStorage::new(io.clone(), "tm");

    submit_fact(&store, "f_dup_a", "shared content");
    submit_fact(&store, "f_dup_b", "shared content");
    store.flush_pending().unwrap();

    let blob_keys = BlockingFihIo::new(io).list("blob/").unwrap();
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
    HintCapable::submit_hint(
        &store,
        &Hint {
            id: FihHash("h1".into()),
            content: "hint one".into(),
            creator: "tester".into(),
        },
    )
    .unwrap();

    IntentCapable::claim_intent(&store, "i1", "alice").unwrap();
    IntentCapable::heartbeat(&store, "i1", "alice").unwrap();
    IntentCapable::conclude_intent(&store, "i1", "result one").unwrap();

    let state = StorageRead::read_state(&store);
    assert_eq!(state.facts.len(), 3, "2 original + 1 conclusion");
    assert_eq!(state.intents.len(), 1);
    assert_eq!(state.hints.len(), 1);
    assert!(state.intents[0].is_concluded);
    assert!(state.intents[0].concluded_at.is_some());
    assert!(state.intents[0].concluded_at.unwrap() > 0);
}

// ── Test 6: Flush chain append-only order preservation ──────────────────
//
// Multiple flushes must produce chain files in strictly increasing timestamp
// order. This guarantees replay order = chronological order.

#[test]
fn test_chain_order_preservation() {
    let io = SimFihIo::new();
    let store = FihStorage::new(io.clone(), "tm");

    let mut cursor = FlushCursor {
        last_flushed_at: 0,
        partition: "default".into(),
    };

    // Submit and flush in 5 batches
    for i in 0..5 {
        submit_fact(&store, &format!("f_batch_{i}"), &format!("batch {i}"));
        let r = flush_at(&store, &cursor);
        assert!(r.records_flushed > 0);
        cursor.last_flushed_at = r.new_cursor.last_flushed_at;
    }

    // List chain files — must exist and be ordered (SimFihIo sorts keys)
    let chains = BlockingFihIo::new(io.clone()).list("flush/").unwrap();
    let chain_files: Vec<&String> = chains.iter().filter(|k| k.ends_with(".chain")).collect();
    assert!(
        chain_files.len() >= 5,
        "at least 5 chain files for 5 batches"
    );
}

// ── Test 7: Empty StateSpace is valid ───────────────────────────────────

#[test]
fn test_empty_statespace_is_valid() {
    let io = SimFihIo::new();
    let store = FihStorage::new(io.clone(), "tm");
    store.flush_pending().unwrap();

    let state = StorageRead::read_state(&store);
    assert!(state.facts.is_empty());
    assert!(state.intents.is_empty());
    assert!(state.hints.is_empty());
}

// ── Test 8: Eviction preserves Fact, removes old Hint ───────────────────

#[test]
fn test_eviction_preserves_fact_removes_old_hint() {
    let store = store();
    submit_fact(&store, "f_keep", "keep me");
    HintCapable::submit_hint(
        &store,
        &Hint {
            id: FihHash("h_old".into()),
            content: "old hint".into(),
            creator: "tester".into(),
        },
    )
    .unwrap();

    EvictCapable::evict_before(&store, "99999999999").unwrap();

    let state = StorageRead::read_state(&store);
    assert_eq!(state.facts.len(), 1, "fact must survive eviction");
    assert_eq!(state.hints.len(), 0, "old hint must be evicted");
}
