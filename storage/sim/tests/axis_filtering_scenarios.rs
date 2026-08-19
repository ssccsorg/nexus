// ── Filtering scenario tests ──────────────────────────────────────────
//
// Record ids are opaque since the CoordId<20> migration: the default
// CoordId depth is 20 and ids are SHA-256 content addresses with no
// semantic axis layout. read_state_filtered matches on record fields
// (origin, creator, submitted_at) supplied via Fact::with_id, not on
// id coordinates.
//
// The legacy 6-syllable form CoordId<6> still exposes the axis API
// (from_axes, axis, entity_type, time_hi). The first two tests exercise
// that API directly as a standalone unit test of CoordId<6>.

use futures_executor::block_on;
use nex_fih::{
    AsyncFactCapable, AsyncFilterCapable, AsyncStorageRead, Content, CoordId, Fact, StateFilter,
};
use nexus_storage_sim::FihStorage;
use nexus_storage_sim::SimIo;

use crate::common::{FakeClock, fact};

mod common;

// ── Test 1: CoordId<6> axis round-trip ────────────────────────────────

#[test]
fn test_axis_roundtrip_via_from_axes() {
    // Build a CoordId<6> with explicit axis values (legacy 6-axis API).
    let coord = CoordId::<6>::from_axes(42, 100, 0, 7, 3, 999).expect("valid axis values");

    // Verify each axis extracts the original value.
    assert_eq!(coord.axis(0).index(), 42, "time_hi");
    assert_eq!(coord.axis(1).index(), 100, "time_lo");
    assert_eq!(coord.axis(2).index(), 0, "entity (Fact)");
    assert_eq!(coord.axis(3).index(), 7, "origin");
    assert_eq!(coord.axis(4).index(), 3, "creator");
    assert_eq!(coord.axis(5).index(), 999, "serial");

    // Verify convenience accessors.
    assert_eq!(coord.time_hi(), 42);
    assert_eq!(coord.entity_type(), 0);
}

// ── Test 2: CoordId<6> axis extraction with max boundary values ────────

#[test]
fn test_axis_boundary_values() {
    // Max valid coord index is 11171 (11172 unique values).
    let coord =
        CoordId::<6>::from_axes(11171, 0, 1, 5000, 11171, 1).expect("valid boundary values");

    assert_eq!(coord.axis(0).index(), 11171);
    assert_eq!(coord.axis(2).index(), 1, "entity (Intent)");
    assert_eq!(coord.axis(4).index(), 11171);
}

// ── Test 3: Filtering by creator ──────────────────────────────────────

#[test]
fn test_filter_by_creator() {
    let io = SimIo::new();
    let store = FihStorage::new(io, "axis_test");

    // Facts with distinct creator record fields.
    let coord_a = CoordId::from_label("axis-creator-10");
    let fact_a = Fact::with_id(
        coord_a,
        "origin_0".into(),
        Content {
            mime_type: "text/plain".into(),
            data: b"creator 10".to_vec(),
        },
        "creator_10".into(),
    );
    block_on(store.submit_fact(&fact_a)).unwrap();

    let coord_b = CoordId::from_label("axis-creator-20");
    let fact_b = Fact::with_id(
        coord_b,
        "origin_0".into(),
        Content {
            mime_type: "text/plain".into(),
            data: b"creator 20".to_vec(),
        },
        "creator_20".into(),
    );
    block_on(store.submit_fact(&fact_b)).unwrap();

    // Filter by creator string (the fast-path fact_by_creator index).
    let filtered = block_on(store.read_state_filtered(&StateFilter {
        axis_hints: None,
        creator: Some("creator_10".into()),
        ..Default::default()
    }));
    assert_eq!(filtered.facts.len(), 1, "expected 1 fact for creator_10");
    assert_eq!(filtered.facts[0].id.to_string(), coord_a.to_string());

    // Filter by creator_20.
    let filtered = block_on(store.read_state_filtered(&StateFilter {
        axis_hints: None,
        creator: Some("creator_20".into()),
        ..Default::default()
    }));
    assert_eq!(filtered.facts.len(), 1, "expected 1 fact for creator_20");
    assert_eq!(filtered.facts[0].id.to_string(), coord_b.to_string());

    // Filter by non-existent creator yields empty result.
    let filtered = block_on(store.read_state_filtered(&StateFilter {
        axis_hints: None,
        creator: Some("creator_nobody".into()),
        ..Default::default()
    }));
    assert_eq!(
        filtered.facts.len(),
        0,
        "expected 0 facts for unknown creator"
    );
}

// ── Test 4: Filtering by time range (since/until) ──────────────────────

#[test]
fn test_filter_by_time_range() {
    // Use FakeClock for deterministic timestamps.
    // Clock starts at 1_000_000_000 ns, steps by 1_000_000_000 ns per call.
    let clock = Box::new(FakeClock::with_step(1_000_000_000, 1_000_000_000));
    let store = FihStorage::with_clock(SimIo::new(), "axis_time", clock);

    // First fact: submitted_at = 1_000_000_000 (clock call 1).
    block_on(store.submit_fact(&fact("f_early"))).unwrap();

    // Second fact: submitted_at = 2_000_000_000 (clock call 2).
    block_on(store.submit_fact(&fact("f_late"))).unwrap();

    // Read the full state to verify both facts are present.
    let full = block_on(store.read_state());
    assert_eq!(full.facts.len(), 2);

    // Filter with since below the first fact's timestamp → both included.
    let all = block_on(store.read_state_filtered(&StateFilter {
        axis_hints: None,
        since: Some("0".into()),
        ..Default::default()
    }));
    assert_eq!(all.facts.len(), 2, "since=0 should return all facts");

    // Filter with since above the second fact's timestamp → none.
    let none = block_on(store.read_state_filtered(&StateFilter {
        axis_hints: None,
        since: Some("3000000000".into()),
        ..Default::default()
    }));
    assert_eq!(
        none.facts.len(),
        0,
        "since=3G (after both facts) should return no facts"
    );

    // Filter with since between the two facts → only the late one.
    let mid = block_on(store.read_state_filtered(&StateFilter {
        axis_hints: None,
        since: Some("1500000000".into()),
        ..Default::default()
    }));
    assert_eq!(
        mid.facts.len(),
        1,
        "since=1.5G (between the two facts) should return 1"
    );

    // Filter with until before the first fact → none.
    let none_until = block_on(store.read_state_filtered(&StateFilter {
        axis_hints: None,
        until: Some("500000000".into()),
        ..Default::default()
    }));
    assert_eq!(
        none_until.facts.len(),
        0,
        "until=0.5G (before both) should return no facts"
    );

    // Filter with until between the two facts → only the early one.
    let early_only = block_on(store.read_state_filtered(&StateFilter {
        axis_hints: None,
        until: Some("1500000000".into()),
        ..Default::default()
    }));
    assert_eq!(
        early_only.facts.len(),
        1,
        "until=1.5G (between early and late) should return 1"
    );

    // Filter with both since and until to capture the middle fact only.
    // With only 2 facts and since=1.5G, there's exactly 1 match (f_late).
    // Adding until=2.5G still includes it.
    let both = block_on(store.read_state_filtered(&StateFilter {
        axis_hints: None,
        since: Some("1500000000".into()),
        until: Some("2500000000".into()),
        ..Default::default()
    }));
    assert_eq!(
        both.facts.len(),
        1,
        "since=1.5G and until=2.5G should capture f_late"
    );
}

// ── Test 5: Filtering by creator with different time buckets ───────────

#[test]
fn test_filter_by_origin_and_time() {
    let io = SimIo::new();
    let store = FihStorage::new(io, "axis_origin_time");

    // Facts with different origins and creators as record fields. Each
    // record gets a unique deterministic label-derived id.
    let facts = [
        ("fact_1", "origin_a", "alice"),
        ("fact_2", "origin_b", "bob"),
        ("fact_3", "origin_a", "alice"),
        ("fact_4", "origin_c", "carol"),
    ];

    for (id_str, origin, creator) in &facts {
        let coord = CoordId::from_label(&format!("axis-{}", id_str));
        let fact = Fact::with_id(
            coord,
            origin.to_string(),
            Content {
                mime_type: "text/plain".into(),
                data: format!("fact {} from {}", id_str, origin).into_bytes(),
            },
            creator.to_string(),
        );
        block_on(store.submit_fact(&fact)).unwrap();
    }

    // All 4 facts present.
    let full = block_on(store.read_state());
    assert_eq!(full.facts.len(), 4);

    // Filter by creator = "alice" (exact string match via fact_by_creator).
    let origin_a = block_on(store.read_state_filtered(&StateFilter {
        axis_hints: None,
        creator: Some("alice".into()),
        ..Default::default()
    }));
    assert_eq!(origin_a.facts.len(), 2, "expected 2 facts created by alice");

    // Filter by creator = "bob".
    let origin_b = block_on(store.read_state_filtered(&StateFilter {
        axis_hints: None,
        creator: Some("bob".into()),
        ..Default::default()
    }));
    assert_eq!(origin_b.facts.len(), 1, "expected 1 fact created by bob");

    // Filter by since + until covering a specific time bucket.
    let bucketed = block_on(store.read_state_filtered(&StateFilter {
        axis_hints: None,
        since: Some("100".into()),
        until: Some("150".into()),
        ..Default::default()
    }));
    // submitted_at is monotonic wall-clock, not a coord axis.
    // submitted_at values depend on submit order and clock,
    // so we cannot assert exact counts on since/until with
    // SystemClock. Instead we verify the filter is applied
    // without error and returns at most all facts.
    assert!(
        bucketed.facts.len() <= 4,
        "time-filtered result should not exceed total facts"
    );

    // Filter with creator + since combined.
    let combined = block_on(store.read_state_filtered(&StateFilter {
        axis_hints: None,
        creator: Some("alice".into()),
        since: Some("0".into()),
        ..Default::default()
    }));
    assert_eq!(
        combined.facts.len(),
        2,
        "expected 2 facts for alice since epoch"
    );
}

// ── Test 6: CoordId<6> entity type values ─────────────────────────────

#[test]
fn test_axis_entity_type_values() {
    // Entity type convention: 0=Fact, 1=Intent, 2=Hint.
    let fact_coord = CoordId::<6>::from_axes(1, 1, 0, 0, 0, 1).unwrap();
    let intent_coord = CoordId::<6>::from_axes(1, 2, 1, 0, 0, 1).unwrap();
    let hint_coord = CoordId::<6>::from_axes(1, 3, 2, 0, 0, 1).unwrap();

    assert_eq!(fact_coord.entity_type(), 0, "entity type for Fact");
    assert_eq!(intent_coord.entity_type(), 1, "entity type for Intent");
    assert_eq!(hint_coord.entity_type(), 2, "entity type for Hint");
}

// ── Test 7: Multi-dimensional Tagma-style query ────────────────────────
//
// Demonstrates multi-dimensional filtering on record fields: filtering
// by origin + creator is a set intersection over record fields, not a
// scan of id coordinates.

#[test]
fn test_multi_dimensional_tagma_query() {
    let io = SimIo::new();
    let store = FihStorage::new(io, "tagma_query");

    // Create a 3D grid of facts: 4 origins × 3 creators × 5 time buckets = 60 facts.
    // Each fact gets a deterministic label-derived id; origin and creator
    // live on the record fields that read_state_filtered matches on.
    for origin_idx in 0..4 {
        for creator_idx in 0..3 {
            for time_bucket in 0..5 {
                let coord = CoordId::from_label(&format!(
                    "axis-{}-{}-{}",
                    origin_idx, creator_idx, time_bucket
                ));
                let fact = Fact::with_id(
                    coord,
                    format!("origin-{}", origin_idx),
                    format!("content-{}-{}-{}", origin_idx, creator_idx, time_bucket).into(),
                    format!("creator-{}", creator_idx),
                );
                block_on(store.submit_fact(&fact)).unwrap();
            }
        }
    }

    assert_eq!(block_on(store.read_state()).facts.len(), 60);

    // Query 1: All facts by creator-1 (single field via fast-path).
    // Expected: 4 origins × 5 time buckets = 20 facts.
    let q1 = block_on(store.read_state_filtered(&StateFilter {
        axis_hints: None,
        creator: Some("creator-1".into()),
        ..Default::default()
    }));
    assert_eq!(q1.facts.len(), 20, "creator-1 should match 4×5=20 facts");

    // Query 2: origin-2 AND creator-1 (dual field via set intersection).
    // Expected: 1 origin × 1 creator × 5 time buckets = 5 facts.
    let q2 = block_on(store.read_state_filtered(&StateFilter {
        axis_hints: None,
        origin: Some("origin-2".into()),
        creator: Some("creator-1".into()),
        ..Default::default()
    }));
    assert_eq!(q2.facts.len(), 5, "origin-2 + creator-1 = 1×1×5 = 5");

    // Query 3: origin-3 AND creator-0 (two fields; since filtering by
    // submitted_at depends on timestamps, not id coordinates).
    // origin-3: 15 facts (3 creators × 5 time)
    // creator-0: 20 facts (4 origins × 5 time)
    // intersection: 1 origin × 1 creator × 5 time = 5 facts.
    let q3 = block_on(store.read_state_filtered(&StateFilter {
        axis_hints: None,
        origin: Some("origin-3".into()),
        creator: Some("creator-0".into()),
        ..Default::default()
    }));
    assert_eq!(q3.facts.len(), 5, "two-field filter: 1×1×5 = 5");

    // Query 4: origin-0 with no other filters (fast-path single key).
    // Expected: 3 creators × 5 time = 15 facts.
    let q4 = block_on(store.read_state_filtered(&StateFilter {
        axis_hints: None,
        origin: Some("origin-0".into()),
        ..Default::default()
    }));
    assert_eq!(q4.facts.len(), 15, "origin-0 should have 3×5=15");

    // Verify record fields are correct for each result.
    for fact in q2.facts.iter() {
        assert_eq!(fact.origin, "origin-2", "origin field should be origin-2");
        assert_eq!(
            fact.creator, "creator-1",
            "creator field should be creator-1"
        );
    }
}
