// Deterministic time-index filter tests using FakeClock.
//
// Tests: since, until (as_of), range (since+until), empty results.
// All timestamps are fully deterministic — no sleep(), no flakiness.

mod common;

use nexus_model::{AsyncFactCapable, AsyncFilterCapable, AsyncStorageRead, FihHash, StateFilter};
use nexus_storage_sim::{FihStorage, SimIo};

fn make_clocked() -> FihStorage<SimIo> {
    FihStorage::with_clock(
        SimIo::new(),
        "test",
        Box::new(common::FakeClock::with_step(1_000_000_000, 1_000_000_000)),
    )
}

#[test]
fn test_since_returns_newer_only() {
    let store = make_clocked();
    futures_executor::block_on(store.submit_fact(&common::fact("f_a"))).unwrap();
    futures_executor::block_on(store.submit_fact(&common::fact("f_b"))).unwrap();

    let state = futures_executor::block_on(store.read_state());
    assert_eq!(state.facts.len(), 2);

    let filtered = futures_executor::block_on(store.read_state_filtered(&StateFilter {
        fact_ids: None,
        intent_ids: None,
        hint_ids: None,
        since: Some("3000000000".into()),
        until: None,
        limit: None,
        offset: None,
        creator: None,
        status: None,
    }));
    assert_eq!(filtered.facts.len(), 1);
    assert_eq!(filtered.facts[0].id, FihHash::from_hex("f_b"));
}

#[test]
fn test_until_as_of_time_travel() {
    let store = make_clocked();
    futures_executor::block_on(store.submit_fact(&common::fact("f_a"))).unwrap();
    futures_executor::block_on(store.submit_fact(&common::fact("f_b"))).unwrap();
    futures_executor::block_on(store.submit_fact(&common::fact("f_c"))).unwrap();

    let filtered = futures_executor::block_on(store.read_state_filtered(&StateFilter {
        fact_ids: None,
        intent_ids: None,
        hint_ids: None,
        since: None,
        until: Some("3000000000".into()),
        limit: None,
        offset: None,
        creator: None,
        status: None,
    }));
    assert_eq!(filtered.facts.len(), 1);
    assert_eq!(filtered.facts[0].id, FihHash::from_hex("f_a"));
}

#[test]
fn test_range_returns_mid_only() {
    let store = make_clocked();
    futures_executor::block_on(store.submit_fact(&common::fact("f_a"))).unwrap();
    futures_executor::block_on(store.submit_fact(&common::fact("f_b"))).unwrap();
    futures_executor::block_on(store.submit_fact(&common::fact("f_c"))).unwrap();

    let filtered = futures_executor::block_on(store.read_state_filtered(&StateFilter {
        fact_ids: None,
        intent_ids: None,
        hint_ids: None,
        since: Some("3000000000".into()),
        until: Some("5000000000".into()),
        limit: None,
        offset: None,
        creator: None,
        status: None,
    }));
    assert_eq!(filtered.facts.len(), 1);
    assert_eq!(filtered.facts[0].id, FihHash::from_hex("f_b"));
}

#[test]
fn test_since_after_all_returns_empty() {
    let store = make_clocked();
    futures_executor::block_on(store.submit_fact(&common::fact("f_a"))).unwrap();
    let filtered = futures_executor::block_on(store.read_state_filtered(&StateFilter {
        fact_ids: None,
        intent_ids: None,
        hint_ids: None,
        since: Some("7000000000".into()),
        until: None,
        limit: None,
        offset: None,
        creator: None,
        status: None,
    }));
    assert_eq!(filtered.facts.len(), 0);
}

#[test]
fn test_until_before_all_returns_empty() {
    let store = make_clocked();
    futures_executor::block_on(store.submit_fact(&common::fact("f_a"))).unwrap();
    let filtered = futures_executor::block_on(store.read_state_filtered(&StateFilter {
        fact_ids: None,
        intent_ids: None,
        hint_ids: None,
        since: None,
        until: Some("1000000000".into()),
        limit: None,
        offset: None,
        creator: None,
        status: None,
    }));
    assert_eq!(filtered.facts.len(), 0);
}

#[test]
fn test_fact_ids_filter_independent_of_time() {
    let store = make_clocked();
    futures_executor::block_on(store.submit_fact(&common::fact("f_a"))).unwrap();
    futures_executor::block_on(store.submit_fact(&common::fact("f_b"))).unwrap();

    let filtered = futures_executor::block_on(store.read_state_filtered(&StateFilter {
        fact_ids: Some(vec!["f_a".into()]),
        intent_ids: None,
        hint_ids: None,
        since: None,
        until: None,
        limit: None,
        offset: None,
        creator: None,
        status: None,
    }));
    assert_eq!(filtered.facts.len(), 1);
    assert_eq!(filtered.facts[0].id, FihHash::from_hex("f_a"));
}

// ── OrderedIndex unit tests (uses u32 compact IDs) ──────────────────────

use nexus_storage_sim::OrderedIndex;

fn idx() -> OrderedIndex<u64> {
    let mut idx = OrderedIndex::new();
    idx.record(100, 1);
    idx.record(200, 2);
    idx.record(300, 3);
    idx
}

#[test]
fn test_record_and_as_of() {
    let idx = idx();

    let at_150 = idx.as_of(&150);
    assert_eq!(at_150.len(), 1);
    assert_eq!(at_150[0].1, 1);

    let at_300 = idx.as_of(&300);
    assert_eq!(at_300.len(), 3);
}

#[test]
fn test_since() {
    let idx = idx();

    let after_150 = idx.since(&150);
    assert_eq!(after_150.len(), 2);
    assert_eq!(after_150[0].1, 2);

    let after_300 = idx.since(&300);
    assert_eq!(after_300.len(), 0);
}

#[test]
fn test_range() {
    let mut idx = idx();
    idx.record(400, 4);

    let mid = idx.range(&150, &350);
    assert_eq!(mid.len(), 2);
    assert_eq!(mid[0].1, 2);
    assert_eq!(mid[1].1, 3);
}

#[test]
fn test_empty() {
    let idx = OrderedIndex::new();
    assert!(idx.is_empty());
    assert_eq!(idx.as_of(&999).len(), 0);
    assert_eq!(idx.since(&0).len(), 0);
}

#[test]
fn test_monotonic_preserved() {
    let mut idx = OrderedIndex::new();
    for i in 0..1000 {
        idx.record((i * 10) as u64, i);
    }
    assert_eq!(idx.len(), 1000);
    let half = idx.as_of(&5000);
    assert_eq!(half.len(), 501);
}
