// Integration tests for nex-derive.
//
// The load-bearing test is `resolve_matches_scan_over_a_population`: the
// resolution is only useful if it returns exactly what the naive predicate
// returns. Every other test guards one property the resolution relies on.

use nex_derive::{AXIS_CARD, Axis, Board, COORD_SPACE, Condition, DeriveOutcome, Query};

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn value(state: &mut u64) -> u8 {
    (splitmix64(state) % AXIS_CARD as u64) as u8
}

fn populate(n: usize, seed: u64) -> Board {
    let mut board = Board::new();
    let mut state = seed;
    while board.len() < n {
        let (work, intent, hint) = (value(&mut state), value(&mut state), value(&mut state));
        board.derive(work, intent, hint, String::new());
    }
    board
}

fn random_query(state: &mut u64) -> Query {
    let mut conditions = Vec::new();
    for axis in Axis::ALL {
        if splitmix64(state).is_multiple_of(3) {
            continue;
        }
        conditions.push(Condition::single(axis, value(state)));
    }
    Query::new(conditions)
}

#[test]
fn resolve_matches_scan_over_a_population() {
    let board = populate(2_000, 1);
    let mut state = 99;
    for _ in 0..200 {
        let query = random_query(&mut state);
        assert_eq!(
            board.resolve(&query),
            board.scan(&query),
            "resolve and scan disagree for {query:?}"
        );
    }
}

#[test]
fn derive_is_idempotent_at_a_goal() {
    let mut board = Board::new();
    assert_eq!(
        board.derive(1, 2, 3, "first".into()),
        DeriveOutcome::Derived
    );
    assert_eq!(
        board.derive(1, 2, 3, "second".into()),
        DeriveOutcome::AlreadyDerived
    );
    assert_eq!(board.len(), 1);
    let coord = nex_derive::pack([1, 2, 3]);
    assert_eq!(board.derivation(coord).unwrap().fact, "first");
}

#[test]
fn unconstrained_query_resolves_every_derivation() {
    let board = populate(500, 7);
    let query = Query::new(vec![
        Condition::all(Axis::Work),
        Condition::all(Axis::Intent),
        Condition::all(Axis::Hint),
    ]);
    assert_eq!(query.constrained_axes(), 0);
    assert_eq!(board.intersection(&query).count(), board.len());
    assert_eq!(board.resolve(&query), board.scan(&query));
}

#[test]
fn resolution_cannot_exceed_its_smallest_posting() {
    let board = populate(3_000, 11);
    let mut state = 5;
    for _ in 0..100 {
        let query = random_query(&mut state);
        if query.constrained_axes() == 0 {
            continue;
        }
        let result = board.intersection(&query).count();
        for condition in &query.conditions {
            if condition.is_unconstrained() {
                continue;
            }
            // The goals carrying an allowed value on this axis alone.
            let posting = board
                .derivations()
                .iter()
                .filter(|derivation| condition.allows(derivation.goal()[condition.axis.index()]))
                .count();
            assert!(
                result <= posting,
                "resolution exceeded a posting for {query:?}"
            );
        }
    }
}

#[test]
fn funnel_is_monotone_nonincreasing() {
    let board = populate(2_000, 17);
    let mut state = 123;
    for _ in 0..100 {
        let query = random_query(&mut state);
        let steps = board.funnel(&query);
        assert_eq!(steps.first().unwrap().1, board.len());
        assert_eq!(steps.last().unwrap().1, board.intersection(&query).count());
        for pair in steps.windows(2) {
            assert!(pair[1].1 <= pair[0].1, "funnel grew for {query:?}");
        }
    }
}

#[test]
fn disjoint_values_resolve_to_empty() {
    let mut board = Board::new();
    board.derive(0, 0, 0, "x".into());
    board.derive(1, 1, 1, "y".into());

    // work=0 and intent=1 never co-occur in this population.
    let query = Query::new(vec![
        Condition::single(Axis::Work, 0),
        Condition::single(Axis::Intent, 1),
    ]);
    assert!(board.intersection(&query).is_empty());
    assert!(board.scan(&query).is_empty());
    assert_eq!(board.resolve(&query), board.scan(&query));
}

#[test]
fn bench_paths_agree() {
    let line = Board::bench(2_000, 50, 42);
    assert!(line.agree, "bench resolution and scan disagreed");
    assert_eq!(line.n, 2_000);
    assert_eq!(line.queries, 50);
}

#[test]
fn pack_round_trips_within_the_space() {
    let mut state = 0;
    for _ in 0..1_000 {
        let values = [value(&mut state), value(&mut state), value(&mut state)];
        let coord = nex_derive::pack(values);
        assert!(coord < COORD_SPACE);
        assert_eq!(nex_derive::unpack(coord), values);
    }
}
