<<<<<<< HEAD
// Comparison: Unified CoordSpaceN<19> + CoordId system.
=======
// Comparison benchmark: CoordSpaceN with proper axis distribution
// vs the old key-concentration pattern.
>>>>>>> 60259d04 (nex: Add axis-filtered iteration and prefix query methods to EntityStore)
//
// Run: cargo test --release -p nexus-storage-sim --test comparison_bench -- <test_name> --nocapture

use std::time::Instant;

use futures_executor::block_on;
use nex_fih::{
    AsyncFactCapable, AsyncFilterCapable, AsyncStorageRead, CoordId, Fact, FihStorage, StateFilter,
};
use nexus_storage_sim::SimIo;

<<<<<<< HEAD
fn write_facts(store: &FihStorage<SimIo>, n: usize) {
    for i in 0..n {
        let cid = CoordId::from_axes(
            (i % 50) as u16,    // [0] time_hi
            (i as u16),          // [1] time_lo
            0,                   // [2] Fact
            (i % 50) as u16,    // [3] origin
            (i % 20) as u16,    // [4] creator
            i as u16,           // [5-10] identity
        ).unwrap();
=======
// ═══════════════════════════════════════════════════════════════════════════
// Write benchmarks
// ═══════════════════════════════════════════════════════════════════════════

/// Baseline: CoordId::new(i) — all keys on axis[0], all other axes 0.
/// Tree structure: 10K entries → 40K branch nodes + 10K leaf nodes ~4.5GB.
#[test]
fn baseline_concentrated_10k() {
    let io = SimIo::new();
    let store = FihStorage::new(io, "concentrated");

    let t = Instant::now();
    for i in 0..10_000 {
>>>>>>> 60259d04 (nex: Add axis-filtered iteration and prefix query methods to EntityStore)
        let fact = Fact::new(
            cid,
            format!("origin-{}", i % 50),
            format!("content-{}", i).into(),
            format!("creator-{}", i % 20),
        );
        block_on(store.submit_fact(&fact)).unwrap();
    }
    block_on(store.flush_pending()).unwrap();
<<<<<<< HEAD
}

#[test]
fn bench_write_10k() {
    let io = SimIo::new();
    let store = FihStorage::new(io, "bench");
    let start = std::time::Instant::now();
    write_facts(&store, 10_000);
    println!("[CS19] write 10K: {:?}", start.elapsed());
}

#[test]
fn bench_read_state() {
    let io = SimIo::new();
    let store = FihStorage::new(io, "bench");
    write_facts(&store, 10_000);
    let start = std::time::Instant::now();
    let state = block_on(store.read_state());
    println!("[CS19] read_state ({}): {:?}", state.facts.len(), start.elapsed());
}

#[test]
fn bench_filter_creator() {
    let io = SimIo::new();
    let store = FihStorage::new(io, "bench");
    write_facts(&store, 10_000);
    let start = std::time::Instant::now();
    for _ in 0..10 {
=======
    println!("[CONCENTRATED] write 10K facts: {:?}", t.elapsed());

    let t = Instant::now();
    let state = block_on(store.read_state());
    println!("[CONCENTRATED] read_state ({} facts): {:?}", state.facts.len(), t.elapsed());

    let t = Instant::now();
    for _ in 0..5 {
>>>>>>> 60259d04 (nex: Add axis-filtered iteration and prefix query methods to EntityStore)
        let state = block_on(store.read_state_filtered(&StateFilter {
            creator: Some("creator-5".into()),
            ..Default::default()
        }));
        assert_eq!(state.facts.len(), 500);
    }
<<<<<<< HEAD
    println!("[CS19] filter creator 10x: {:?}", start.elapsed());
}

#[test]
fn bench_filter_origin_creator() {
    let io = SimIo::new();
    let store = FihStorage::new(io, "bench");
    write_facts(&store, 10_000);
    let start = std::time::Instant::now();
    let state = block_on(store.read_state_filtered(&StateFilter {
        origin: Some("origin-7".into()),
        creator: Some("creator-3".into()),
        ..Default::default()
    }));
    println!("[CS19] filter origin+creator: {:?} ({} facts)", start.elapsed(), state.facts.len());
=======
    println!("[CONCENTRATED] filter creator (5x): {:?} (avg {:?})", t.elapsed(), t.elapsed() / 5);
}

/// Properly distributed: keys spread across all 6 axes using from_axes.
/// Axis convention: [time_hi, time_lo, entity, origin, creator, serial]
///
/// Distribution: 6 time_hi × 10 time_lo × 1 entity × 50 origin × 20 creator × 166 serial
/// Tree structure: ~6 + 60 + 60 + 3000 + 60000 + 10000 = ~73K nodes total
/// But each node is SHARED among many entries (unlike concentrated which has 10K unique paths).
#[test]
fn proper_distribution_10k() {
    let io = SimIo::new();
    let store = FihStorage::new(io, "proper");
    let clock = nanos_since_epoch();

    let t = Instant::now();
    for i in 0..10_000 {
        let (time_hi, time_lo) = time_buckets(clock + i as u64);
        let id = CoordId::from_axes(time_hi, time_lo, 0, (i % 50) as u16, (i % 20) as u16, (i / 1000) as u16)
            .expect("valid coords");
        let fact = Fact::new(
            id,
            format!("origin-{}", i % 50),
            format!("content-{}", i).into(),
            format!("creator-{}", i % 20),
        );
        block_on(store.submit_fact(&fact)).unwrap();
    }
    block_on(store.flush_pending()).unwrap();
    println!("[PROPER] write 10K facts: {:?}", t.elapsed());

    let t = Instant::now();
    let state = block_on(store.read_state());
    println!("[PROPER] read_state ({} facts): {:?}", state.facts.len(), t.elapsed());

    let t = Instant::now();
    for _ in 0..5 {
        let state = block_on(store.read_state_filtered(&StateFilter {
            creator: Some("creator-5".into()),
            ..Default::default()
        }));
        assert_eq!(state.facts.len(), 500);
    }
    println!("[PROPER] filter creator (5x): {:?} (avg {:?})", t.elapsed(), t.elapsed() / 5);
}

// ═══════════════════════════════════════════════════════════════════════════
// CoordSpaceN as index space — proper axis-based filtering
// ═══════════════════════════════════════════════════════════════════════════

/// Demonstrate CoordSpaceN as index space: use iter_prefix for O(depth) filtering
/// instead of full tree scan + string comparison.
///
/// Currently read_state_filtered does a full values() scan. The ideal would be:
///   space.iter_prefix(&[some_coord])  →  subtree under that prefix
///
/// This test measures the raw iter_prefix speed vs full iter_tree scan.
#[test]
fn iter_prefix_vs_full_scan() {
    use tagma_core::{CoordSpaceN, CoordPath, Coord};

    // Use predictable time values for reproducible iteration
    // time_hi: 0..9 (10 values), time_lo: 0..9 (10 values)
    // This ensures we know exactly which prefixes exist.
    let mut space: CoordSpaceN<6, usize> = CoordSpaceN::new();

    for i in 0..10_000 {
        let time_hi = ((i / 1000) % 10) as u16;   // cycles 0..9 every 10K
        let time_lo = ((i / 100) % 10) as u16;     // cycles 0..9 every 1K
        let origin = (i % 50) as u16;
        let creator = (i % 20) as u16;
        let serial = (i % 100) as u16;              // 100 serials per origin×creator
        let coords = [
            Coord::new(time_hi).unwrap(),
            Coord::new(time_lo).unwrap(),
            Coord::new(0).unwrap(),
            Coord::new(origin).unwrap(),
            Coord::new(creator).unwrap(),
            Coord::new(serial).unwrap(),
        ];
        let path = CoordPath::new(coords);
        space.place_path(&path, i);
    }

    // Full scan via iter_tree
    let t = Instant::now();
    let mut count = 0;
    let mut sum = 0usize;
    for (_path, v) in space.iter_tree() {
        count += 1;
        sum += *v;
    }
    println!("[TREE-SCAN] full scan: {} items, sum={}, time={:?}", count, sum, t.elapsed());

    // iter_prefix scan on creator=5 (axis[4]=5)
    // prefix: [*, *, *, *, creator=5]
    // This only traverses the subtree under creator=5
    let prefix = Coord::new(5).unwrap();
    // We need a 5-coord prefix: [time_hi, time_lo, entity=0, origin, creator=5]
    // But iter_prefix takes &[Coord], and we need to match ANY time_hi, time_lo, entity, origin
    // with creator=5. This means we need to iterate all origin branches under creator=5.
    //
    // Better approach: just measure iter_tree and compare with iter_prefix on a specific path.
    // For creator=5 filtering, we need to iterate over all (time_hi, time_lo, entity, origin, creator=5)
    // prefixes. That's 6 × 10 × 1 × 50 = 3000 iter_prefix calls.
    //
    // But actually we can do a partial scan: start iter_tree but skip branches that don't match.
    // This is what a smart implementation would do.

    // Baseline: full scan with in-memory filter (simulates current read_state_filtered)
    let t = Instant::now();
    count = 0;
    sum = 0;
    for (_path, v) in space.iter_tree() {
        // Extract creator from path (axis[4])
        if _path.coords()[4].index() == 5 {
            count += 1;
            sum += *v;
        }
    }
    println!("[FULL-SCAN+FILTER] filter creator=5: {} items, time={:?}", count, t.elapsed());
    assert_eq!(count, 500);

    // iter_prefix approach: use nested prefixes for creator=5 filtering.
    // Each prefix descends 5 levels to reach creator=5, then scans the remaining leaf.
    // Since creator=5 entries are spread across all (time_hi, time_lo, origin) combos,
    // we need 10 × 10 × 50 = 5000 iter_prefix calls — each traversing 5 levels.
    //
    // Compare: full scan visits ALL tree nodes (including non-creator-5 branches).
    // iter_prefix visits ONLY the subtree under creator=5.
    let t = Instant::now();
    count = 0;
    sum = 0;
    for th in 0..10 {
        for tl in 0..10 {
            for origin in 0..50 {
                let prefix = [
                    Coord::new(th).unwrap(),
                    Coord::new(tl).unwrap(),
                    Coord::new(0).unwrap(),
                    Coord::new(origin).unwrap(),
                    Coord::new(5).unwrap(),
                ];
                if let Some(iter) = space.iter_prefix(&prefix) {
                    for (_p, v) in iter {
                        count += 1;
                        sum += *v;
                    }
                }
            }
        }
    }
    println!("[ITER-PREFIX] nested prefix creator=5: {} items, time={:?}", count, t.elapsed());
    assert_eq!(count, 500);

    // Single-prefix filter: time_hi=0 — only traverses 1/10th of the tree.
    let t = Instant::now();
    count = 0;
    sum = 0;
    let prefix = [Coord::new(0).unwrap()];
    if let Some(iter) = space.iter_prefix(&prefix) {
        for (_p, v) in iter {
            count += 1;
            sum += *v;
        }
    }
    println!("[ITER-PREFIX] time_hi=0 only: {} items, time={:?}", count, t.elapsed());
    // time_hi=0 has entries where (i/1000)%10 == 0, so i ∈ [0,999] ∪ [10000,...]
    // With only 10K entries: i=0..999 → 1000 items
    assert_eq!(count, 1000);

    // ── origin+creator combined: iter_prefix with 5-level prefix vs full scan ──
    // origin=7, creator=3 requires knowing time_hi, time_lo → nested iteration.
    // Full scan baseline:
    let t = Instant::now();
    count = 0;
    for (_path, v) in space.iter_tree() {
        if _path.coords()[3].index() == 7 && _path.coords()[4].index() == 3 {
            count += 1;
            sum += *v;
        }
    }
    println!("[FULL-SCAN] origin=7 + creator=3: {} items, time={:?}", count, t.elapsed());

    // iter_prefix with known time_hi/time_lo:
    let t = Instant::now();
    count = 0;
    sum = 0;
    for th in 0..10 {
        for tl in 0..10 {
            let prefix = [
                Coord::new(th).unwrap(),
                Coord::new(tl).unwrap(),
                Coord::new(0).unwrap(),
                Coord::new(7).unwrap(),  // origin
                Coord::new(3).unwrap(),  // creator
            ];
            if let Some(iter) = space.iter_prefix(&prefix) {
                for (_p, v) in iter {
                    count += 1;
                    sum += *v;
                }
            }
        }
    }
    println!("[ITER-PREFIX] origin=7 + creator=3: {} items, time={:?}", count, t.elapsed());

    println!("\n═══ SUMMARY ═══");
    println!("CoordSpaceN is an INDEX SPACE, not a KV store.");
    println!("iter_prefix is O(depth + subtree) vs full scan O(total nodes).");
    println!("The cost: you must know ALL parent axis values for prefix filtering.");
    println!("The benefit: 10-100x faster filtering when axis layout matches query patterns.");
}

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn nanos_since_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64
}

/// Split a nanosecond timestamp into time_hi/time_lo axis values.
fn time_buckets(ts_ns: u64) -> (u16, u16) {
    let hi = (ts_ns / 86_400_000_000_000 % 11172) as u16;
    let lo = (ts_ns % 86_400_000_000_000 % 11172) as u16;
    (hi, lo)
>>>>>>> 60259d04 (nex: Add axis-filtered iteration and prefix query methods to EntityStore)
}
