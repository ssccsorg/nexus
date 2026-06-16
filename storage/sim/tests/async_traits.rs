// ── Async trait integration tests ──────────────────────────────────────
//
// Tests that AsyncStorageRead, AsyncFactCapable, etc. delegate correctly
// on top of SimIo (in-memory, no R2). Uses `futures_executor::block_on`
// to drive async in test context (native only, not WASM).

use nexus_model::{
    AsyncEvictCapable, AsyncFactCapable, AsyncFilterCapable, AsyncFlushCapable, AsyncHintCapable,
    AsyncIntentCapable, AsyncScanCapable, AsyncStorageRead, AsyncTimeRangeCapable, FihHash,
    FlushCursor, Hint, StateFilter,
};
use nexus_storage_sim::FihStorage;
use nexus_storage_sim::SimIo;

mod common;

fn setup() -> FihStorage<SimIo> {
    let io = SimIo::new();
    let clock = Box::new(common::FakeClock::new(1_000_000_000));
    FihStorage::with_clock(io, "test", clock)
}

// ── AsyncStorageRead + AsyncFactCapable ────────────────────────────────

#[test]
fn test_async_submit_and_read_fact() {
    let store = setup();
    let fact = common::fact("f1");

    let hash = futures_executor::block_on(store.submit_fact(&fact)).unwrap();
    assert_eq!(hash, FihHash::from_hex("f1"));

    let state = futures_executor::block_on(store.read_state());
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.facts[0].id, FihHash::from_hex("f1"));
}

#[test]
fn test_async_submit_multiple_facts() {
    let store = setup();
    for i in 0..3 {
        let f = common::fact(&format!("f{i}"));
        futures_executor::block_on(store.submit_fact(&f)).unwrap();
    }

    let state = futures_executor::block_on(store.read_state());
    assert_eq!(state.facts.len(), 3);
}

// ── AsyncHintCapable ──────────────────────────────────────────────────

#[test]
fn test_async_submit_hint() {
    let store = setup();
    let hint = Hint {
        id: FihHash::from_hex("h1"),
        content: "test hint".into(),
        creator: "t".into(),
    };
    futures_executor::block_on(store.submit_hint(&hint)).unwrap();

    let state = futures_executor::block_on(store.read_state());
    assert_eq!(state.hints.len(), 1);
}

// ── AsyncIntentCapable lifecycle ──────────────────────────────────────

#[test]
fn test_async_claim_intent() {
    let store = setup();
    let f = common::fact("f_claim");
    futures_executor::block_on(store.submit_fact(&f)).unwrap();
    let intent = common::intent("i_claim", vec!["f_claim"]);
    futures_executor::block_on(store.submit_intent(&intent)).unwrap();

    futures_executor::block_on(store.claim_intent("i_claim", "agent1")).unwrap();

    // Claimed by someone else should conflict
    let err = futures_executor::block_on(store.claim_intent("i_claim", "agent2"));
    assert!(err.is_err());
}

#[test]
fn test_async_conclude_intent() {
    let store = setup();
    let f = common::fact("f_conc");
    futures_executor::block_on(store.submit_fact(&f)).unwrap();
    let intent = common::intent("i_conc", vec!["f_conc"]);
    futures_executor::block_on(store.submit_intent(&intent)).unwrap();
    futures_executor::block_on(store.claim_intent("i_conc", "agent1")).unwrap();

    let result = futures_executor::block_on(store.conclude_intent("i_conc", "done"));
    assert!(result.is_ok());
    let fact = result.unwrap();
    assert!(fact.id.to_string().starts_with("f_concl_"));
}

#[test]
fn test_async_conclude_unclaimed_fails() {
    let store = setup();
    let f = common::fact("f_unc");
    futures_executor::block_on(store.submit_fact(&f)).unwrap();
    let intent = common::intent("i_unc", vec!["f_unc"]);
    futures_executor::block_on(store.submit_intent(&intent)).unwrap();

    // Not claimed → conclude should fail
    let err = futures_executor::block_on(store.conclude_intent("i_unc", "done"));
    assert!(err.is_err());
}

#[test]
fn test_async_submit_intent() {
    let store = setup();
    // Need a fact first
    let f = common::fact("f_base");
    futures_executor::block_on(store.submit_fact(&f)).unwrap();

    let intent = common::intent("i1", vec!["f_base"]);
    let hash = futures_executor::block_on(store.submit_intent(&intent)).unwrap();
    assert_eq!(hash, FihHash::from_hex("i1"));
}

// ── AsyncFilterCapable (delegates to sync) ────────────────────────────

#[test]
fn test_async_filter() {
    let store = setup();
    for i in 0..5 {
        let mut f = common::fact(&format!("f{i}"));
        f.origin = if i < 3 { "early".into() } else { "late".into() };
        futures_executor::block_on(store.submit_fact(&f)).unwrap();
    }

    let filter = StateFilter {
        fact_ids: Some(vec!["f0".into(), "f2".into()]),
        ..Default::default()
    };
    let state = futures_executor::block_on(store.read_state_filtered(&filter));
    assert_eq!(state.facts.len(), 2);
}

// ── AsyncEvictCapable (delegates to sync) ─────────────────────────────

#[test]
fn test_async_evict() {
    let store = setup();
    let hint = Hint {
        id: FihHash::from_hex("h_old"),
        content: "old".into(),
        creator: "t".into(),
    };
    futures_executor::block_on(store.submit_hint(&hint)).unwrap();

    let size = futures_executor::block_on(store.approximate_size());
    assert!(size > 0);
}

// ── AsyncScanCapable (delegates to sync) ──────────────────────────────

#[test]
fn test_async_scan() {
    let store = setup();
    let mut f = common::fact("f_scan");
    f.origin = "partition:p1".into();
    futures_executor::block_on(store.submit_fact(&f)).unwrap();

    let data = futures_executor::block_on(store.scan_partition("p1")).unwrap();
    assert_eq!(data.facts.len(), 1);
}

// ── AsyncTimeRangeCapable (delegates to sync) ─────────────────────────

#[test]
fn test_async_time_range() {
    let store = setup();
    futures_executor::block_on(store.submit_fact(&common::fact("f_tr"))).unwrap();

    let range = futures_executor::block_on(store.time_range());
    assert!(range.is_some());
}

// ── AsyncFlushCapable ─────────────────────────────────────────────────

#[test]
fn test_async_flush_empty() {
    let store = setup();
    let cursor = FlushCursor {
        last_flushed_at: 0,
        partition: "test".into(),
    };
    let result = futures_executor::block_on(store.flush_since(&cursor)).unwrap();
    assert_eq!(result.records_flushed, 0);
}

#[test]
fn test_async_flush_with_records() {
    let store = setup();
    futures_executor::block_on(store.submit_fact(&common::fact("f_flush"))).unwrap();

    let cursor = FlushCursor {
        last_flushed_at: 0,
        partition: "test".into(),
    };
    let result = futures_executor::block_on(store.flush_since(&cursor)).unwrap();
    assert_eq!(result.records_flushed, 1);
}
