// Deterministic time-index filter tests using FakeClock.
//
// Tests: since, until (as_of), range (since+until), empty results.
// All timestamps are fully deterministic — no sleep(), no flakiness.

mod common;

use nexus_model::{FactCapable, FilterCapable, StateFilter, StorageRead};
use nexus_storage_sim::{FihStorage, SimIo};

fn make_clocked() -> FihStorage<SimIo> {
    FihStorage::with_clock(
        SimIo::new(),
        "test",
        Box::new(common::FakeClock::with_step(1_000_000_000, 1_000_000_000)), // start at 1s, step 1s
    )
}

#[test]
fn test_since_returns_newer_only() {
    let store = make_clocked();
    FactCapable::submit_fact(&store, &common::fact("f_a")).unwrap(); // ts=10_000_000
    FactCapable::submit_fact(&store, &common::fact("f_b")).unwrap(); // ts=11_000_000

    let state = StorageRead::read_state(&store);
    assert_eq!(state.facts.len(), 2);

    // since=3_000_000_000 → only f_b (ts=4G) and f_c (ts=6G)
    let filtered = store.read_state_filtered(&StateFilter {
        fact_ids: None,
        intent_ids: None,
        hint_ids: None,
        since: Some("3000000000".into()),
        until: None,
        limit: None,
        offset: None,
    });
    assert_eq!(filtered.facts.len(), 1);
    assert_eq!(filtered.facts[0].id.0, "f_b");
}

#[test]
fn test_until_as_of_time_travel() {
    let store = make_clocked();
    FactCapable::submit_fact(&store, &common::fact("f_a")).unwrap(); // ts=10_000_000
    FactCapable::submit_fact(&store, &common::fact("f_b")).unwrap(); // ts=11_000_000
    FactCapable::submit_fact(&store, &common::fact("f_c")).unwrap(); // ts=12_000_000

    // f_a at ~2G, f_b at ~4G, f_c at ~6G
    // as_of=3_000_000_000 → only f_a (ts=2G)
    let filtered = store.read_state_filtered(&StateFilter {
        fact_ids: None,
        intent_ids: None,
        hint_ids: None,
        since: None,
        until: Some("3000000000".into()),
        limit: None,
        offset: None,
    });
    assert_eq!(filtered.facts.len(), 1);
    assert_eq!(filtered.facts[0].id.0, "f_a");
}

#[test]
fn test_range_returns_mid_only() {
    let store = make_clocked();
    FactCapable::submit_fact(&store, &common::fact("f_a")).unwrap(); // ts=10_000_000
    FactCapable::submit_fact(&store, &common::fact("f_b")).unwrap(); // ts=11_000_000
    FactCapable::submit_fact(&store, &common::fact("f_c")).unwrap(); // ts=12_000_000

    // f_a at ~2G, f_b at ~4G, f_c at ~6G
    // range 3_000_000_000..5_000_000_000 → f_b only
    let filtered = store.read_state_filtered(&StateFilter {
        fact_ids: None,
        intent_ids: None,
        hint_ids: None,
        since: Some("3000000000".into()),
        until: Some("5000000000".into()),
        limit: None,
        offset: None,
    });
    assert_eq!(filtered.facts.len(), 1);
    assert_eq!(filtered.facts[0].id.0, "f_b");
}

#[test]
fn test_since_after_all_returns_empty() {
    let store = make_clocked();
    FactCapable::submit_fact(&store, &common::fact("f_a")).unwrap(); // ts=10_000_000

    // f_a at ~2G
    // since=7_000_000_000 → empty
    let filtered = store.read_state_filtered(&StateFilter {
        fact_ids: None,
        intent_ids: None,
        hint_ids: None,
        since: Some("7000000000".into()),
        until: None,
        limit: None,
        offset: None,
    });
    assert_eq!(filtered.facts.len(), 0);
}

#[test]
fn test_until_before_all_returns_empty() {
    let store = make_clocked();
    FactCapable::submit_fact(&store, &common::fact("f_a")).unwrap(); // ts=10_000_000

    // f_a at ~2G
    // as_of=1_000_000_000 → empty (nothing submitted yet)
    let filtered = store.read_state_filtered(&StateFilter {
        fact_ids: None,
        intent_ids: None,
        hint_ids: None,
        since: None,
        until: Some("1000000000".into()),
        limit: None,
        offset: None,
    });
    assert_eq!(filtered.facts.len(), 0);
}

#[test]
fn test_fact_ids_filter_independent_of_time() {
    let store = make_clocked();
    FactCapable::submit_fact(&store, &common::fact("f_a")).unwrap();
    FactCapable::submit_fact(&store, &common::fact("f_b")).unwrap();

    // fact_ids filter (no time filter) → only f_a
    let filtered = store.read_state_filtered(&StateFilter {
        fact_ids: Some(vec!["f_a".into()]),
        intent_ids: None,
        hint_ids: None,
        since: None,
        until: None,
        limit: None,
        offset: None,
    });
    assert_eq!(filtered.facts.len(), 1);
    assert_eq!(filtered.facts[0].id.0, "f_a");
}

// ── TimeIndex unit tests ──────────────────────────────────────────────────

use nexus_storage_sim::index::TimeIndex;

#[test]
fn test_record_and_as_of() {
    let idx = TimeIndex::new();
    idx.record(100, "f001");
    idx.record(200, "f002");
    idx.record(300, "f003");

    let at_150 = idx.as_of(150);
    assert_eq!(at_150.len(), 1);
    assert_eq!(at_150[0].1, "f001");

    let at_300 = idx.as_of(300);
    assert_eq!(at_300.len(), 3);
}

#[test]
fn test_since() {
    let idx = TimeIndex::new();
    idx.record(100, "f001");
    idx.record(200, "f002");
    idx.record(300, "f003");

    let after_150 = idx.since(150);
    assert_eq!(after_150.len(), 2);
    assert_eq!(after_150[0].1, "f002");

    let after_300 = idx.since(300);
    assert_eq!(after_300.len(), 0);
}

#[test]
fn test_range() {
    let idx = TimeIndex::new();
    idx.record(100, "f001");
    idx.record(200, "f002");
    idx.record(300, "f003");
    idx.record(400, "f004");

    let mid = idx.range(150, 350);
    assert_eq!(mid.len(), 2);
    assert_eq!(mid[0].1, "f002");
    assert_eq!(mid[1].1, "f003");
}

#[test]
fn test_empty() {
    let idx = TimeIndex::new();
    assert!(idx.is_empty());
    assert_eq!(idx.as_of(999).len(), 0);
    assert_eq!(idx.since(0).len(), 0);
}

#[test]
fn test_monotonic_preserved() {
    let idx = TimeIndex::new();
    // Simulate sequential timestamps
    for i in 0..1000 {
        idx.record((i * 10) as u64, &format!("f{:04}", i));
    }
    assert_eq!(idx.len(), 1000);
    // as_of at midpoint
    let half = idx.as_of(5000);
    assert_eq!(half.len(), 501); // 0..=500 inclusive
}
