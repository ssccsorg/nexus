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
    AsyncFactCapable, AsyncFilterCapable, AsyncIntentCapable, AsyncStorageRead, CoordId, Fact,
    FihStorage, Intent, StateFilter,
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
            vec![
                CoordId::from_axes(
                    (i % N_TIMEBUCKETS) as u16,
                    0,
                    0,
                    (i % N_ORIGINS) as u16,
                    (i % N_CREATORS) as u16,
                    i as u16,
                )
                .unwrap(),
            ],
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
            let f1 = StateFilter {
                origin: Some("origin-10".into()),
                creator: Some("creator-5".into()),
                ..Default::default()
            };
            let state = block_on(store.read_state_filtered(black_box(&f1)));
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
                let store = FihStorage::with_clock(io, "write", Box::new(nex_core::SystemClock));
                for i in 0..10_000 {
                    let cid =
                        CoordId::from_axes(0, 0, 0, (i % 50) as u16, (i % 20) as u16, i as u16)
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

// ── Real scenario: research knowledge base query ──────────────────
//
// Simulates a researcher querying their AI knowledge base:
// "Show me all documents from the 'paradigm' project by author 'alice'
//  from last week's session, and find related intents."
//
// In the old system (HashMap + FihCoord), this requires:
// 1. full-text scan or separate index for each dimension
// 2. O(N) filter for each additional dimension
// 3. separate intent index lookup
//
// With CoordId axes, all dimensions are encoded in the storage address itself.

fn build_knowledge_base() -> FihStorage<SimIo> {
    let io = SimIo::new();
    let store = FihStorage::with_clock(io, "kb", Box::new(nex_core::SystemClock));

    // 10K documents across 10 projects, 20 authors, 50 time buckets
    for i in 0..10_000 {
        let cid = CoordId::from_axes(
            (i / 2000) as u16,     // [0] time: every 2000 docs = 1 time bucket
            (i % 2000) as u16,      // [1] sequence within bucket
            0,                      // [2] entity: Fact
            (i % 10) as u16,        // [3] origin: project (0..9)
            ((i / 1000) % 20) as u16, // [4] creator: author (0..19)
            i as u16,               // [5] serial
        ).unwrap();
        let fact = Fact::new(
            cid,
            format!("project-{}", i % 10),
            format!("Document {}: research content about paradigm shift", i).into(),
            format!("author-{}", (i / 1000) % 20),
        );
        block_on(store.submit_fact(&fact)).unwrap();
    }

    // 1000 intents referencing the last 1000 facts
    for i in 0..1000 {
        let fact_idx = 10_000 - 1000 + i; // last 1000 facts
        let cid = CoordId::from_axes(
            (fact_idx / 2000) as u16,
            (fact_idx % 2000) as u16,
            0,
            (fact_idx % 10) as u16,
            ((fact_idx / 1000) % 20) as u16,
            fact_idx as u16,
        ).unwrap();
        let intent_id = CoordId::from_axes(50, i as u16, 1, 0, 0, (200_000 + i) as u16).unwrap();
        let intent = Intent::new(
            intent_id,
            vec![cid],
            None,
            format!("analyze document {}", fact_idx),
            "detector".into(),
        );
        block_on(store.submit_intent(&intent)).unwrap();
    }

    block_on(store.flush_pending()).unwrap();
    store
}

fn bench_knowledge_base_query(c: &mut Criterion) {
    let store = build_knowledge_base();

    let mut group = c.benchmark_group("kb_query");

    // Q1: documents from project-3 by author-7
    group.bench_function("project_author", |b| {
        b.iter(|| {
            let state = block_on(store.read_state_filtered(&StateFilter {
                origin: Some("project-3".into()),
                creator: Some("author-7".into()),
                ..Default::default()
            }));
            // project-3: 10K docs, author-7: 5K docs
            // intersection: 500 docs (every 20th author in project-3)
            black_box(state);
        });
    });

    // Q2: all documents from project-5 (single-axis)
    group.bench_function("project_only", |b| {
        b.iter(|| {
            let state = block_on(store.read_state_filtered(&StateFilter {
                origin: Some("project-5".into()),
                ..Default::default()
            }));
            black_box(state);
        });
    });

    // Q3: intents referencing specific facts (last 100 facts)
    group.bench_function("intents_by_fact", |b| {
        b.iter(|| {
            for i in 0..100 {
                let fidx = 10_000 - 1000 + i * 10;
                let cid = CoordId::from_axes(
                    (fidx / 2000) as u16, (fidx % 2000) as u16, 0,
                    (fidx / 10000) as u16, ((fidx / 1000) % 20) as u16,
                    fidx as u16
                ).unwrap();
                black_box(store.intents_by_fact(&cid.to_string()));
            }
        });
    });

    // Q4: time-bucketed project query
    group.bench_function("project_time_range", |b| {
        b.iter(|| {
            let state = block_on(store.read_state_filtered(&StateFilter {
                origin: Some("project-2".into()),
                creator: Some("author-5".into()),
                since: Some("1".into()),
                until: Some("99999999999999".into()),
                ..Default::default()
            }));
            black_box(state);
        });
    });

    group.finish();
}

// ── Compare: what the old system would need ─────────────────────────
//
// The `kb_query/project_author` benchmark filters 100K docs by origin AND creator.
// In the OLD system (HashMap + FihCoord), querying by origin required:
//   - FihCoord::fact_ids_by_origin() → HashMap lookup O(1)
//   - then intersect with FihCoord::facts_by_creator() O(min(set1, set2))
//   - then materialize each Fact from MemoryEntityStore O(results)
//
// In the NEW system:
//   - fact_by_origin HashMap lookup O(1)
//   - fact_by_creator HashMap lookup O(1)
//   - HashSet intersection O(min(set1, set2))
//   - materialize from CoordEntityStore O(results)
//
// The bottleneck in BOTH systems is HashSet allocation and Fact materialization.
// The CoordEntityStore removes SHA-256 hashing per lookup, but the
// fast-path tables remain the same structure as the old FihCoord index.
//
// Key difference: NEW system has ZERO index maintenance on write
// (CoordSpaceN is self-indexing). OLD system updated 9 FihCoord structures
// per write.

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
        bench_knowledge_base_query,
);
criterion_main!(benches);
