//! nexus Criterion benchmarks — Tagma multi-axis index vs legacy HashMap intersection.
//!
//! Run: cargo bench -p nex
//!
//! Key design:
//!   - exist:         all axis values exist + the combination exists
//!   - nonexist_real: all axis values exist individually but the COMBINATION does not
//!   - nonexist_fake: at least one axis value does not exist at all (short-circuit)
//!   - Scaling:       10K, 100K, 500K entries to show Tagma flat vs Legacy linear

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use nex::storage::core::index::FihCoord;
use nexus_model::FihHash;

// ---------------------------------------------------------------------------
// Seed helpers
// ---------------------------------------------------------------------------

/// Fill `count` facts across `n_origins x n_creators` coordinate grid.
/// When count < n_origins * n_creators, some combos are deliberately unfilled.
fn seed_coord(count: usize, n_origins: usize, n_creators: usize) -> FihCoord {
    let coord = FihCoord::new();
    for i in 0..count {
        let tag = format!("f{i:06x}");
        let origin = format!("org-{}", i % n_origins);
        let creator = format!("cr-{}", (i / n_origins) % n_creators);
        let id = FihHash::from_hex(&tag);
        coord.record_fact(&id.0, &origin, &creator, (i * 100) as u64);
    }
    coord
}

const EXIST_ORIGIN: &str = "org-0";
const EXIST_CREATOR: &str = "cr-0";

// All seeded layouts:
//
//   10K:  100 origins x 100 creators = 100% fill  →  exist only
//   100K: 200 origins x 500 creators = 100% fill  →  exist only
//   500K: 500 origins x 2000 creators = 50% fill  →  exist + nonexist_real
//
// For 500K layout:
//   Each origin gets 500000/500 = 1000 entries spread across cr-0..cr-999.
//   cr-1999 exists in the index (from other origins) but NOT for org-0.

const N_ORIGINS_500K: usize = 500;
const N_CREATORS_500K: usize = 2000;

fn nonexist_real_origin() -> &'static str { "org-0" }
fn nonexist_real_creator() -> String {
    // org-0 has entries cr-0..cr-999. cr-1999 exists but not for org-0.
    format!("cr-{}", N_CREATORS_500K - 1)
}

// ---------------------------------------------------------------------------
// Legacy intersection (manual, no-branch)
// ---------------------------------------------------------------------------

fn legacy_2axis_count(coord: &FihCoord, origin: &str, creator: &str) -> usize {
    let by_o = coord.fact_ids_by_origin(origin);
    let by_c = coord.facts_by_creator(creator);
    let set: std::collections::HashSet<u32> =
        std::collections::HashSet::from_iter(by_c.iter().copied());
    by_o.iter().filter(|id| set.contains(id)).count()
}

// ── 10K scale ──────────────────────────────────────────────────────────

fn bench_10k(c: &mut Criterion) {
    let coord = seed_coord(10_000, 100, 100);
    let mut g = c.benchmark_group("scale_10k");
    g.throughput(criterion::Throughput::Elements(1));

    g.bench_function("tagma_exist", |b| {
        b.iter(|| { black_box(coord.by_origin_creator(black_box(EXIST_ORIGIN), black_box(EXIST_CREATOR))); });
    });
    // nonexist_fake: origin exists, creator doesn't (short-circuit in both)
    g.bench_function("tagma_nonexist_fake", |b| {
        b.iter(|| { black_box(coord.by_origin_creator(black_box(EXIST_ORIGIN), black_box("cr-NONEXIST"))); });
    });
    g.bench_function("legacy_exist", |b| {
        b.iter(|| { black_box(legacy_2axis_count(&coord, black_box(EXIST_ORIGIN), black_box(EXIST_CREATOR))); });
    });
    // Legacy 2-axis nonexist uses same fallback path as Tagma when creator absent.
    g.bench_function("legacy_nonexist_fake", |b| {
        b.iter(|| { black_box(legacy_2axis_count(&coord, black_box(EXIST_ORIGIN), black_box("cr-NONEXIST"))); });
    });

    g.finish();
}

// ── 100K scale ─────────────────────────────────────────────────────────

fn bench_100k(c: &mut Criterion) {
    let coord = seed_coord(100_000, 200, 500);
    let mut g = c.benchmark_group("scale_100k");
    g.throughput(criterion::Throughput::Elements(1));

    g.bench_function("tagma_exist", |b| {
        b.iter(|| { black_box(coord.by_origin_creator(black_box(EXIST_ORIGIN), black_box(EXIST_CREATOR))); });
    });
    g.bench_function("legacy_exist", |b| {
        b.iter(|| { black_box(legacy_2axis_count(&coord, black_box(EXIST_ORIGIN), black_box(EXIST_CREATOR))); });
    });
    g.finish();
}

// ── 500K scale (50% fill) ──────────────────────────────────────────────

fn bench_500k(c: &mut Criterion) {
    let coord = seed_coord(500_000, N_ORIGINS_500K, N_CREATORS_500K);
    let nonexist_o = nonexist_real_origin();
    let nonexist_c = nonexist_real_creator();
    let mut g = c.benchmark_group("scale_500k");
    g.throughput(criterion::Throughput::Elements(1));

    // Tagma: 2 array accesses, flat regardless of data volume
    g.bench_function("tagma_exist", |b| {
        b.iter(|| { black_box(coord.by_origin_creator(black_box(EXIST_ORIGIN), black_box(EXIST_CREATOR))); });
    });
    g.bench_function("tagma_nonexist_real", |b| {
        b.iter(|| { black_box(coord.by_origin_creator(black_box(&nonexist_o), black_box(&nonexist_c))); });
    });

    // Legacy exist: Vec(1000) ∩ Vec(250) = O(min(n,m)) scan
    g.bench_function("legacy_exist", |b| {
        b.iter(|| { black_box(legacy_2axis_count(&coord, black_box(EXIST_ORIGIN), black_box(EXIST_CREATOR))); });
    });
    // Legacy nonexist_real: STILL fetches Vec(1000) + Vec(250) and intersects
    g.bench_function("legacy_nonexist_real", |b| {
        b.iter(|| {
            let by_o = coord.fact_ids_by_origin(black_box(&nonexist_o));
            let by_c = coord.facts_by_creator(black_box(&nonexist_c));
            let set: std::collections::HashSet<u32> =
                std::collections::HashSet::from_iter(by_c.iter().copied());
            // Iterates all 1000 entries, 250 HashSet lookups → waste
            let mut n = 0usize;
            for id in by_o.iter() {
                if set.contains(id) {
                    n += 1;
                }
            }
            black_box(n);
        });
    });

    g.finish();
}

// ── Baseline: single HashMap lookup ────────────────────────────────────

fn bench_single_axis(c: &mut Criterion) {
    let coord = seed_coord(100_000, 200, 500);
    let mut g = c.benchmark_group("single_axis_hashmap");
    g.throughput(criterion::Throughput::Elements(1));

    g.bench_function("by_origin", |b| {
        b.iter(|| { black_box(coord.fact_ids_by_origin(black_box("org-42"))); });
    });
    g.bench_function("by_creator", |b| {
        b.iter(|| { black_box(coord.facts_by_creator(black_box("cr-7"))); });
    });

    g.finish();
}

// ── Criterion harness ──────────────────────────────────────────────────

criterion_group!(
    benches,
    bench_10k,
    bench_100k,
    bench_500k,
    bench_single_axis,
);
criterion_main!(benches);
