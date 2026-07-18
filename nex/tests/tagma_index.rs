use nexus_model::FihHash;
use nex::storage::core::index::{intersect_2, intersect_3, FihCoord};

fn make_coord() -> FihCoord {
    FihCoord::new()
}

fn record_fact(coord: &FihCoord, tag: &str, origin: &str, creator: &str, ts: u64) {
    let id = FihHash::from_hex(tag);
    coord.record_fact(&id.0, origin, creator, ts);
}

// ── Tagma query: by_origin_creator ───────────────────────────────────

#[test]
fn tagma_query_2axis_exact_match() {
    let coord = make_coord();
    record_fact(&coord, "f001", "origin-a", "creator-x", 100);

    let result = coord.by_origin_creator("origin-a", "creator-x");
    let expected = coord.intern(&FihHash::from_hex("f001").0);
    assert_eq!(result, vec![expected]);
}

#[test]
fn tagma_query_2axis_no_match_returns_empty() {
    let coord = make_coord();
    record_fact(&coord, "f001", "origin-a", "creator-x", 100);

    let result = coord.by_origin_creator("origin-a", "creator-z");
    assert!(result.is_empty());
}

// ── Tagma query: by_origin_creator_status ────────────────────────────

#[test]
fn tagma_query_3axis_origin_mismatch_returns_empty() {
    let coord = make_coord();
    record_fact(&coord, "f001", "origin-a", "creator-x", 100);

    let result = coord.by_origin_creator_status("origin-z", "creator-x", "submitted");
    assert!(result.is_empty());
}

#[test]
fn tagma_query_3axis_creator_mismatch_returns_empty() {
    let coord = make_coord();
    record_fact(&coord, "f001", "origin-a", "creator-x", 100);

    let result = coord.by_origin_creator_status("origin-a", "creator-z", "submitted");
    assert!(result.is_empty());
}

// ── Tagma fallback: matches legacy intersection ──────────────────────

#[test]
fn tagma_fallback_2axis_matches_legacy() {
    let coord = make_coord();
    record_fact(&coord, "f001", "origin-a", "creator-x", 100);
    record_fact(&coord, "f002", "origin-a", "creator-y", 200);
    record_fact(&coord, "f003", "origin-b", "creator-x", 300);

    let tagma = coord.by_origin_creator("origin-a", "creator-x");
    let legacy = intersect_2(
        &coord.fact_ids_by_origin("origin-a"),
        &coord.facts_by_creator("creator-x"),
    );
    assert_eq!(tagma, legacy, "Tagma fast path must match legacy intersection");
}

// ── intersect helpers ────────────────────────────────────────────────

#[test]
fn intersect_2_basic() {
    let a = vec![1, 2, 3, 4];
    let b = vec![3, 4, 5, 6];
    let mut result = intersect_2(&a, &b);
    result.sort();
    assert_eq!(result, vec![3, 4]);
}

#[test]
fn intersect_2_empty() {
    assert!(intersect_2(&[], &[1, 2, 3]).is_empty());
    assert!(intersect_2(&[1, 2, 3], &[]).is_empty());
}

#[test]
fn intersect_2_no_overlap() {
    assert!(intersect_2(&[1, 2], &[3, 4]).is_empty());
}

#[test]
fn intersect_3_basic() {
    let a = vec![1, 2, 3, 4];
    let b = vec![2, 3, 4, 5];
    let c = vec![3, 4, 5, 6];
    let mut result = intersect_3(&a, &b, &c);
    result.sort();
    assert_eq!(result, vec![3, 4]);
}

#[test]
fn intersect_3_empty() {
    assert!(intersect_3(&[], &[1, 2], &[2, 3]).is_empty());
}

#[test]
fn intersect_3_no_overlap() {
    assert!(intersect_3(&[1], &[2], &[3]).is_empty());
}
