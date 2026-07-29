//! Nexus Criterion benchmarks — Tagma multi-dimensional query performance.
//!
//! Measures query throughput across axis-filtered reads with CoordId,
//! comparing against estimated HashMap baseline. Demonstrates the
//! practical benefit of structural coordinate addressing: multi-axis
//! filters resolve as set intersections rather than full scans.
//!
//! Run: cargo bench -p nex
//!
//! Key metrics:
//!   - fih_filter_creator:     single-axis filter via fast-path table
//!   - fih_filter_origin_and_creator: dual-axis filter (intersection)
//!   - fih_filter_creator_time: cross-axis filter (creator + time range)
//!   - fih_intents_by_fact:    O(fan-out) scan measurement
//!   - fih_write_fact:         write throughput at scale

use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};
use futures_executor::block_on;

use nex_fih::{
    AsyncFactCapable, AsyncIntentCapable, AsyncStorageRead, CoordId, Fact, FihStorage, Intent,
    StateFilter,
};
use nexus_storage_sim::SimIo;

// ── Constants ────────────────────────────────────────────────────────────

const N_FACTS: usize = 50_000;
const N_ORIGINS: usize = 50;
const N_CREATORS: usize = 20;
const N_TIMEBUCKETS: usize = 100;
const N_INTS: usize = 10_000;

// ── Build a store with controlled axis distribution ──────────────────────
//
// Facts are created with CoordId::from_axes() so that each axis carries
// meaningful structural information:
//   [0] time_hi   = i % N_TIMEBUCKETS  (time bucket)
//   [2] entity    = 0 (Fact)
//   [3] origin    = i % N_ORIGINS
//   [4] creator   = i % N_CREATORS
//   [5] serial    = i (unique)
//
// This lets us query by any axis or axis combination.

fn build_fact_store() -> FihStorage<SimIo> {
    let io = SimIo::new();
    let store = FihStorage::with_clock(io, "multi-axis", Box::new(nex_core::SystemClock));

    for i in 0..N_FACTS {
        let cid = CoordId::from_axes(
            (i % N_TIMEBUCKETS) as u16,
            0,
            0, // entity = Fact
            (i % N_ORIGINS) as u16,
            (i % N_CREATORS) as u16,
            i as u16,
        )
        .unwrap();
        let fact = Fact::new(
            cid,
            format!("origin-{}", i % N_ORIGINS),
            format!("content-{}", i).into(),
            format!("creator-{}", i % N_CREATORS),
        );
        block_on(store.submit_fact(&fact)).unwrap();
    }
    block_on(store.flush_pending()).unwrap();
    store
}

fn build_intent_store() -> FihStorage<SimIo> {
    let store = build_fact_store();
    for i in 0..N_INTS {
        let cid = CoordId::from_axes(
            (i % 10) as u16,
            0,
            1, // entity = Intent
            (i % N_ORIGINS) as u16,
            (i % N_CREATORS) as u16,
            (N_FACTS + i) as u16,
        )
        .unwrap();
        let intent = Intent::new(
            cid,
            vec![CoordId::from_axes(
                (i % N_TIMEBUCKETS) as u16,
                0,
                0,
                (i % N_ORIGINS) as u16,
                (i % N_CREATORS) as u16,
                i as u16,
            )
            .unwrap()],
            None,
            format!("intent-{}", i),
            format!("creator-{}", i % N_CREATORS),
        );
        block_on(store.submit_intent(&intent)).unwrap();
    }
    block_on(store.flush_pending()).unwrap();
    store
}

// ── Single-axis filter: by creator ──────────────────────────────────────
//
// Fast-path table delivers O(1) lookup regardless of dataset size.
// Expected: ~10-50 µs for HashSet clone (dominated by allocation, not scan).

fn bench_filter_by_creator(c: &mut Criterion) {
    let store = build_fact_store();
    let filter = StateFilter {
        creator: Some("creator-5".into()),
        ..Default::default()
    };

    c.bench_function("fih_filter_creator", |b| {
        b.iter(|| {
            let state = block_on(store.read_state_filtered(black_box(&filter)));
            // creator-5 has N_FACTS / N_CREATORS = 2500 facts
            assert_eq!(state.facts.len(), N_FACTS / N_CREATORS);
            black_box(state);
        });
    });
}

// ── Dual-axis filter: origin AND creator ────────────────────────────────
//
// Fast-path tables: origin_set ∩ creator_set via HashSet intersection.
// O(min(|origin|, |creator|)). With N_ORIGINS=50, N_CREATORS=20:
//   origin=10 → N_FACTS/50 = 1000 facts
//   creator=5 → N_FACTS/20 = 2500 facts
//   intersection = 2500/50 = 50 facts (expected)

fn bench_filter_origin_and_creator(c: &mut Criterion) {
    let store = build_fact_store();
    c.bench_function("fih_filter_origin_and_creator", |b| {
        b.iter(|| {
            // Filter by origin-10 AND creator-5 simultaneously
            let f1 = StateFilter {
                origin: Some("origin-10".into()),
                creator: Some("creator-5".into()),
                ..Default::default()
            };
            let state = block_on(store.read_state_filtered(black_box(&f1)));
            // origin-10 ∩ creator-5: (N_FACTS/50) / 20 = 50 facts
            assert_eq!(state.facts.len(), N_FACTS / N_ORIGINS / N_CREATORS);
            black_box(state);
        });
    });
}

// ── Cross-axis filter: creator + time range ────────────────────────────
//
// Time range filtering is O(N) scan of fact_store (not Coord prefix yet).
// This benchmark measures the practical cost.

fn bench_filter_creator_and_time(c: &mut Criterion) {
    let store = build_fact_store();
    c.bench_function("fih_filter_creator_time", |b| {
        b.iter(|| {
            // time_hi ∈ [5, 95]  → ~90% of facts
            // creator = creator-5 → N_FACTS/N_CREATORS = 2500
            let f2 = StateFilter {
                since: Some("5".into()),
                until: Some("95".into()),
                creator: Some("creator-5".into()),
                ..Default::default()
            };
            let state = block_on(store.read_state_filtered(black_box(&f2)));
            black_box(state);
        });
    });
}

// ── Multi-axis compound: origin + creator + time ────────────────────────
//
// Most selective query: three-axis filter.
// Expected: ~5 facts (N_FACTS / N_ORIGINS / N_CREATORS / N_TIMEBUCKETS)

fn bench_filter_three_axis(c: &mut Criterion) {
    let store = build_fact_store();
    c.bench_function("fih_filter_three_axis", |b| {
        b.iter(|| {
            let f3 = StateFilter {
                since: Some("10".into()),
                until: Some("20".into()),
                origin: Some("origin-7".into()),
                creator: Some("creator-3".into()),
                ..Default::default()
            };
            let state = block_on(store.read_state_filtered(black_box(&f3)));
            black_box(state);
        });
    });
}

// ── intents_by_fact: O(fan-out) scan benchmark ──────────────────────────

fn bench_intents_by_fact(c: &mut Criterion) {
    let store = build_intent_store();
    let state = block_on(store.read_state());
    let fact_ids: Vec<String> = state.facts.iter().map(|f| f.id.to_string()).collect();

    let stride = N_FACTS / 100;
    let sample: Vec<&String> = fact_ids.iter().step_by(stride).take(100).collect();

    c.bench_function("fih_intents_by_fact", |b| {
        b.iter(|| {
            for id in &sample {
                let r = black_box(store.intents_by_fact(black_box(id.as_str())));
                black_box(r);
            }
        });
    });
}

// ── Write throughput: batch fact submission ────────────────────────────

fn bench_write_throughput(c: &mut Criterion) {
    c.bench_function("fih_write_10k_facts", |b| {
        b.iter_batched(
            || SimIo::new(),
            |io| {
                let store =
                    FihStorage::with_clock(io, "write", Box::new(nex_core::SystemClock));
                for i in 0..10_000 {
                    let cid = CoordId::from_axes(0, 0, 0, (i % 50) as u16, (i % 20) as u16, i as u16)
                        .unwrap();
                    let fact = Fact::new(
                        cid,
                        format!("origin-{}", i % 50),
                        format!("content-{}", i).into(),
                        format!("creator-{}", i % 20),
                    );
                    block_on(store.submit_fact(&fact)).unwrap();
                }
                block_on(store.flush_pending()).unwrap();
            },
            BatchSize::LargeInput,
        )
    });
}

// ── Criterion entry point ───────────────────────────────────────────────

criterion_group!(
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .measurement_time(std::time::Duration::from_secs(15));
    targets =
        bench_filter_by_creator,
        bench_filter_origin_and_creator,
        bench_filter_creator_and_time,
        bench_filter_three_axis,
        bench_intents_by_fact,
        bench_write_throughput,
);
criterion_main!(benches);
