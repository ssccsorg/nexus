// Deterministic time-index filter tests using FakeClock.
//
// Tests: since, until (as_of), range (since+until), empty results.
// All timestamps are fully deterministic — no sleep(), no flakiness.

mod common;

use nexus_model::{Content, Fact, FactCapable, FihHash, FilterCapable, StateFilter, StorageRead};
use nexus_storage_sim::{NativeFihStorage, SimFihIo};

fn make_clocked() -> NativeFihStorage<SimFihIo> {
    NativeFihStorage::with_clock(
        SimFihIo::new(),
        "test",
        Box::new(common::FakeClock::with_step(1_000_000_000, 1_000_000_000)), // start at 1s, step 1s
    )
}

fn fact(id: &str, data: &[u8]) -> Fact {
    Fact {
        id: FihHash(id.into()),
        origin: "t".into(),
        content: Content {
            mime_type: "text/plain".into(),
            data: data.to_vec(),
        },
        creator: "t".into(),
    }
}

#[test]
fn test_since_returns_newer_only() {
    let store = make_clocked();
    FactCapable::submit_fact(&store, &fact("f_a", b"a")).unwrap(); // ts=10_000_000
    FactCapable::submit_fact(&store, &fact("f_b", b"b")).unwrap(); // ts=11_000_000

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
    FactCapable::submit_fact(&store, &fact("f_a", b"a")).unwrap(); // ts=10_000_000
    FactCapable::submit_fact(&store, &fact("f_b", b"b")).unwrap(); // ts=11_000_000
    FactCapable::submit_fact(&store, &fact("f_c", b"c")).unwrap(); // ts=12_000_000

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
    FactCapable::submit_fact(&store, &fact("f_a", b"a")).unwrap(); // ts=10_000_000
    FactCapable::submit_fact(&store, &fact("f_b", b"b")).unwrap(); // ts=11_000_000
    FactCapable::submit_fact(&store, &fact("f_c", b"c")).unwrap(); // ts=12_000_000

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
    FactCapable::submit_fact(&store, &fact("f_a", b"a")).unwrap(); // ts=10_000_000

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
    FactCapable::submit_fact(&store, &fact("f_a", b"a")).unwrap(); // ts=10_000_000

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
    FactCapable::submit_fact(&store, &fact("f_a", b"a")).unwrap();
    FactCapable::submit_fact(&store, &fact("f_b", b"b")).unwrap();

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
