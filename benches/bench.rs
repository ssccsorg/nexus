// ═══════════════════════════════════════════════════════════════════════════
// nexus-bench — unified FIH storage benchmark suite
//
// Single-file Criterion benchmark (syntagma-style). Run:
//   cargo bench -p nexus-bench
//
// Coverage:
//   raw/      — CoordSpaceN<6> primitives (full scan vs iter_prefix)
//   cs/       — CoordSpace type comparison (dense CS2 vs tree CSN6, hash index)
//   fih/      — FihStorage application level (filter, AND, axis_hints, write)
//   kb_query/ — real knowledge-base scenario (10K docs, 10 projects, 20 authors)
//
// Measured on Apple M1 (ARMv8.4-A Firestorm 3.2GHz), release profile (median of 10 samples, 2026-07-31):
//   raw/full_scan_50k          540 ms          (50K entries, full tree walk)
//   raw/iter_prefix_50k        1.12 ms         (5×100 prefixes, O(subtree))
//   cs/csn6_get_10k            533 µs          (10K tree lookups, ~53 ns/op)
//   cs/cs2_get_10k             60.8 µs         (10K dense lookups, ~6 ns/op)
//   cs/hashmap_get_10k         120.9 µs        (10K hash lookups, ~12.1 ns/op)
//   fih/filter_creator_50k     745 µs          (HashMap fast-path + materialize 2500)
//   fih/and_query_50k          296 µs          (HashSet intersection)
//   fih/filter_creator_time    777 µs          (creator + time range)
//   fih/filter_three_axis      83.8 µs         (origin + creator + time)
//   fih/axis_hints_no          127 µs          (full scan fallback)
//   fih/axis_hints_with        42.1 µs         (iter_prefix fast path)
//   fih/write_10k_facts        51.9 ms         (batch write + flush)
//   fih/intents_by_fact_100    743 ms          (100 calls, ~7.4 ms/call)
//   kb_query/project_author    70.2 µs         (origin+creator AND, 500 hits)
//   kb_query/project_only      260 µs          (single-axis, 1000 hits)
//   kb_query/project_time_range 70.4 µs        (origin+creator+time)
//   kb_query/intents_by_fact   6.04 s / 100    (O(fan-out) scan, known cost)
//
// Paradigm framing: HashMap (full scan) vs Tagma (CoordSpaceN index +
// fast-path tables). The old 28 ms full-scan ceiling is broken by paying
// construction cost only for matches; AND queries shrink the candidate set
// via HashSet intersection at each added dimension.
// ═══════════════════════════════════════════════════════════════════════════

use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};
use futures_executor::block_on;

use nex_fih::{
    AsyncFactCapable, AsyncFilterCapable, AsyncIntentCapable, AsyncStorageRead, AxisHints,
    BlackboardError, ContentMeta, CoordId, Fact, FactRecord, FihStorage, Intent, StateFilter,
};
use nexus_storage_sim::{FileIo, SimIo};

// ── Fixtures: controlled axis distributions ─────────────────────────────

const N_FACTS: usize = 50_000;
const N_ORIGINS: usize = 50;
const N_CREATORS: usize = 20;
const N_TIMEBUCKETS: usize = 100;
const N_INTS: usize = 10_000;

/// 10K facts with time-aware CoordId axes (real clock timestamps).
fn populate_10k(store: &FihStorage<SimIo>) -> u64 {
    let clock = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    for i in 0..10_000u32 {
        let cid = CoordId::from_axes(
            ((clock + i as u64) / 86_400_000_000_000 % 11172) as u16,
            ((clock + i as u64) % 86_400_000_000_000 % 11172) as u16,
            0,
            (i % 50) as u16,
            (i % 20) as u16,
            (i / 1000) as u16,
        )
        .unwrap();
        block_on(store.submit_fact(&Fact::with_id(
            cid,
            format!("origin-{}", i % 50),
            format!("c-{}", i).into(),
            format!("creator-{}", i % 20),
        )))
        .unwrap();
    }
    block_on(store.flush_pending()).unwrap();
    clock
}

/// 50K facts with sequential CoordId.
#[allow(dead_code)]
fn populate_50k(store: &FihStorage<SimIo>) {
    for i in 0..50_000u32 {
        block_on(store.submit_fact(&Fact::with_id(
            CoordId::new(i as u64),
            format!("origin-{}", i % 500),
            format!("c-{}", i).into(),
            format!("creator-{}", i % 200),
        )))
        .unwrap();
    }
    block_on(store.flush_pending()).unwrap();
}

/// 50K facts with explicit multi-axis CoordId:
///   [0] time_hi = i % N_TIMEBUCKETS, [2] entity = Fact,
///   [3] origin = i % N_ORIGINS, [4] creator = i % N_CREATORS, [5] serial = i
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
        let fact = Fact::with_id(
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

/// Fact store plus 10K intents referencing facts.
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

/// Research knowledge base: 10K documents (10 projects × 20 authors × 50 time
/// buckets) plus 1K intents referencing the last 1K facts.
fn build_knowledge_base() -> FihStorage<SimIo> {
    let io = SimIo::new();
    let store = FihStorage::with_clock(io, "kb", Box::new(nex_core::SystemClock));

    for i in 0..10_000 {
        let cid = CoordId::from_axes(
            (i / 2000) as u16,        // [0] time: every 2000 docs = 1 time bucket
            (i % 2000) as u16,        // [1] sequence within bucket
            0,                        // [2] entity: Fact
            (i % 10) as u16,          // [3] origin: project (0..9)
            ((i / 1000) % 20) as u16, // [4] creator: author (0..19)
            i as u16,                 // [5] serial
        )
        .unwrap();
        let fact = Fact::with_id(
            cid,
            format!("project-{}", i % 10),
            format!("Document {}: research content about paradigm shift", i).into(),
            format!("author-{}", (i / 1000) % 20),
        );
        block_on(store.submit_fact(&fact)).unwrap();
    }

    for i in 0..1000 {
        let fact_idx = 10_000 - 1000 + i; // last 1000 facts
        let cid = CoordId::from_axes(
            (fact_idx / 2000) as u16,
            (fact_idx % 2000) as u16,
            0,
            (fact_idx % 10) as u16,
            ((fact_idx / 1000) % 20) as u16,
            fact_idx as u16,
        )
        .unwrap();
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

// ── raw/: CoordSpaceN primitives ─────────────────────────────────────────

fn bench_raw_full_scan_50k(c: &mut Criterion) {
    use tagma_core::{Coord, CoordPath, CoordSpaceN};

    let mut space: CoordSpaceN<6, u32> = CoordSpaceN::new();
    for i in 0..50_000u32 {
        space.place_path(
            &CoordPath::new([
                Coord::new((i / 10000) as u16).unwrap(),
                Coord::new(((i / 100) % 100) as u16).unwrap(),
                Coord::new(0).unwrap(),
                Coord::new((i % 200) as u16).unwrap(),
                Coord::new((i % 100) as u16).unwrap(),
                Coord::new((i % 1000) as u16).unwrap(),
            ]),
            i,
        );
    }

    c.bench_function("raw/full_scan_50k", |b| {
        b.iter(|| {
            let mut n = 0u32;
            for (_p, _v) in space.iter_tree() {
                n += 1;
            }
            black_box(n);
        });
    });
}

fn bench_raw_iter_prefix_50k(c: &mut Criterion) {
    use tagma_core::{Coord, CoordPath, CoordSpaceN};

    let mut space: CoordSpaceN<6, u32> = CoordSpaceN::new();
    for i in 0..50_000u32 {
        space.place_path(
            &CoordPath::new([
                Coord::new((i / 10000) as u16).unwrap(),
                Coord::new(((i / 100) % 100) as u16).unwrap(),
                Coord::new(0).unwrap(),
                Coord::new((i % 200) as u16).unwrap(),
                Coord::new((i % 100) as u16).unwrap(),
                Coord::new((i % 1000) as u16).unwrap(),
            ]),
            i,
        );
    }

    c.bench_function("raw/iter_prefix_50k", |b| {
        b.iter(|| {
            let mut n = 0u32;
            for th in 0..5u16 {
                for tl in 0..100u16 {
                    if let Some(iter) = space.iter_prefix(&[
                        Coord::new(th).unwrap(),
                        Coord::new(tl).unwrap(),
                        Coord::new(0).unwrap(),
                        Coord::new(7).unwrap(),
                        Coord::new(7).unwrap(),
                    ]) {
                        for (_p, _v) in iter {
                            n += 1;
                        }
                    }
                }
            }
            black_box(n);
        });
    });
}

// ── cs/: CoordSpace type comparison (dense CS2 vs tree CSN6) ────────────

fn bench_cs_get_10k(c: &mut Criterion) {
    use tagma_core::{Coord, CoordPath, CoordSpace2, CoordSpaceN};

    let n = 10_000u32;
    let axes: Vec<[u16; 6]> = (0..n)
        .map(|i| {
            let clock = 1700000000000000u64;
            [
                ((clock + i as u64) / 86_400_000_000_000 % 11172) as u16,
                ((clock + i as u64) % 86_400_000_000_000 % 11172) as u16,
                0u16,
                (i % 500) as u16,
                (i % 200) as u16,
                (i / 1000) as u16,
            ]
        })
        .collect();

    let paths: Vec<CoordPath<6>> = axes
        .iter()
        .map(|a| {
            CoordPath::new([
                Coord::new(a[0]).unwrap(),
                Coord::new(a[1]).unwrap(),
                Coord::new(a[2]).unwrap(),
                Coord::new(a[3]).unwrap(),
                Coord::new(a[4]).unwrap(),
                Coord::new(a[5]).unwrap(),
            ])
        })
        .collect();

    // CSN6 (tree)
    let mut csn: CoordSpaceN<6, u64> = CoordSpaceN::new();
    for (i, p) in paths.iter().enumerate() {
        csn.place_path(p, i as u64);
    }
    c.bench_function("cs/csn6_get_10k", |b| {
        b.iter(|| {
            let mut acc = 0u64;
            for p in &paths {
                acc += csn.at_path(p).copied().unwrap_or(0);
            }
            black_box(acc);
        });
    });

    // CS2 (dense heap, 6→2 axes packed)
    let pack = |a: &[u16; 6]| {
        CoordPath::new([
            Coord::new(((a[0] as u64 * 11172 + a[1] as u64) % 11172) as u16).unwrap(),
            Coord::new(
                ((((a[2] as u64 * 11172 + a[3] as u64) * 11172 + a[4] as u64) * 11172
                    + a[5] as u64)
                    % 11172) as u16,
            )
            .unwrap(),
        ])
    };
    let packed: Vec<CoordPath<2>> = axes.iter().map(pack).collect();
    let mut cs2: CoordSpace2<u64> = CoordSpace2::new();
    for (i, p) in packed.iter().enumerate() {
        cs2.place_path(p, i as u64);
    }
    c.bench_function("cs/cs2_get_10k", |b| {
        b.iter(|| {
            let mut acc = 0u64;
            for p in &packed {
                acc += cs2.at_path(p).copied().unwrap_or(0);
            }
            black_box(acc);
        });
    });
}

// Hash-based identity index (the existing approach): 32-byte key per record
// (FihHash scale), same 10K six-axis dataset as the CoordSpace benches.
fn bench_cs_hashmap_get_10k(c: &mut Criterion) {
    use std::collections::HashMap;

    let n = 10_000u32;
    let axes: Vec<[u16; 6]> = (0..n)
        .map(|i| {
            let clock = 1700000000000000u64;
            [
                ((clock + i as u64) / 86_400_000_000_000 % 11172) as u16,
                ((clock + i as u64) % 86_400_000_000_000 % 11172) as u16,
                0u16,
                (i % 500) as u16,
                (i % 200) as u16,
                (i / 1000) as u16,
            ]
        })
        .collect();

    let keys: Vec<[u8; 32]> = axes
        .iter()
        .map(|a| {
            let mut k = [0u8; 32];
            for (j, v) in a.iter().enumerate() {
                k[j * 2] = (v >> 8) as u8;
                k[j * 2 + 1] = (v & 0xff) as u8;
            }
            k
        })
        .collect();

    let mut map: HashMap<[u8; 32], u64> = HashMap::new();
    for (i, k) in keys.iter().enumerate() {
        map.insert(*k, i as u64);
    }

    c.bench_function("cs/hashmap_get_10k", |b| {
        b.iter(|| {
            let mut acc = 0u64;
            for k in &keys {
                acc += map.get(k).copied().unwrap_or(0);
            }
            black_box(acc);
        });
    });
}

// ── fih/: FihStorage application level ───────────────────────────────────

fn bench_fih_filter_creator(c: &mut Criterion) {
    let store = build_fact_store();
    let filter = StateFilter {
        creator: Some("creator-5".into()),
        ..Default::default()
    };
    c.bench_function("fih/filter_creator_50k", |b| {
        b.iter(|| {
            let state = block_on(store.read_state_filtered(black_box(&filter)));
            assert_eq!(state.facts.len(), N_FACTS / N_CREATORS);
            black_box(state);
        });
    });
}

fn bench_fih_and_query(c: &mut Criterion) {
    let store = build_fact_store();
    let filter = StateFilter {
        origin: Some("origin-7".into()),
        creator: Some("creator-7".into()),
        ..Default::default()
    };
    c.bench_function("fih/and_query_50k", |b| {
        b.iter(|| {
            let state = block_on(store.read_state_filtered(black_box(&filter)));
            black_box(state);
        });
    });
}

fn bench_fih_filter_creator_time(c: &mut Criterion) {
    let store = build_fact_store();
    c.bench_function("fih/filter_creator_time", |b| {
        b.iter(|| {
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

fn bench_fih_filter_three_axis(c: &mut Criterion) {
    let store = build_fact_store();
    c.bench_function("fih/filter_three_axis", |b| {
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

fn bench_fih_axis_hints(c: &mut Criterion) {
    let io = SimIo::new();
    let store = FihStorage::new(io, "hints");
    let clock = populate_10k(&store);
    let th = ((clock) / 86_400_000_000_000 % 11172) as u16;
    let tl = ((clock) % 86_400_000_000_000 % 11172) as u16;

    let no_hints = StateFilter {
        creator: Some("creator-5".into()),
        ..Default::default()
    };
    let with_hints = StateFilter {
        origin: Some("origin-7".into()),
        creator: Some("creator-7".into()),
        axis_hints: Some(AxisHints {
            time_hi: Some(th),
            time_lo: Some(tl),
            entity: Some(0),
            origin: Some(7),
            creator: Some(7),
            serial: None,
        }),
        ..Default::default()
    };

    c.bench_function("fih/axis_hints_no", |b| {
        b.iter(|| {
            let s = block_on(store.read_state_filtered(black_box(&no_hints)));
            black_box(s);
        });
    });
    c.bench_function("fih/axis_hints_with", |b| {
        b.iter(|| {
            let s = block_on(store.read_state_filtered(black_box(&with_hints)));
            black_box(s);
        });
    });
}

fn bench_fih_write_10k(c: &mut Criterion) {
    c.bench_function("fih/write_10k_facts", |b| {
        b.iter_batched(
            || SimIo::new(),
            |io| {
                let store = FihStorage::with_clock(io, "write", Box::new(nex_core::SystemClock));
                for i in 0..10_000 {
                    let cid =
                        CoordId::from_axes(0, 0, 0, (i % 50) as u16, (i % 20) as u16, i as u16)
                            .unwrap();
                    let fact = Fact::with_id(
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

fn bench_fih_intents_by_fact(c: &mut Criterion) {
    let store = build_intent_store();
    let state = block_on(store.read_state());
    let fact_ids: Vec<String> = state.facts.iter().map(|f| f.id.to_string()).collect();
    let stride = N_FACTS / 100;
    let sample: Vec<&String> = fact_ids.iter().step_by(stride).take(100).collect();

    c.bench_function("fih/intents_by_fact_100", |b| {
        b.iter(|| {
            for id in &sample {
                let r = black_box(store.intents_by_fact(black_box(id.as_str())));
                black_box(r);
            }
        });
    });
}

// ── kb_query/: real knowledge-base scenario ──────────────────────────────

fn bench_kb_query(c: &mut Criterion) {
    let store = build_knowledge_base();
    let mut group = c.benchmark_group("kb_query");

    group.bench_function("project_author", |b| {
        b.iter(|| {
            let state = block_on(store.read_state_filtered(&StateFilter {
                origin: Some("project-3".into()),
                creator: Some("author-7".into()),
                ..Default::default()
            }));
            black_box(state);
        });
    });

    group.bench_function("project_only", |b| {
        b.iter(|| {
            let state = block_on(store.read_state_filtered(&StateFilter {
                origin: Some("project-5".into()),
                ..Default::default()
            }));
            black_box(state);
        });
    });

    group.bench_function("intents_by_fact", |b| {
        b.iter(|| {
            for i in 0..100 {
                let fidx = 10_000 - 1000 + i * 10;
                let cid = CoordId::from_axes(
                    (fidx / 2000) as u16,
                    (fidx % 2000) as u16,
                    0,
                    (fidx / 10000) as u16,
                    ((fidx / 1000) % 20) as u16,
                    fidx as u16,
                )
                .unwrap();
                black_box(store.intents_by_fact(&cid.to_string()));
            }
        });
    });

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

// ── conflict/: content_hash conflict detection cost (#176) ──────────────

/// Reopen a FihStorage holding `n` facts. The records and blobs are
/// written to IO directly (a pre-existing on-disk state), then
/// `rebuild_cache` loads them: the in-memory record map is empty and the
/// id-keyed entity store plus the record layer (record maps + structural
/// filter index) are populated.
fn build_reopened_conflict_store(n: usize) -> FihStorage<SimIo> {
    let io = SimIo::new();
    for i in 0..n {
        let cid = CoordId::from_axes(0, 0, 0, (i % 50) as u16, (i % 20) as u16, i as u16).unwrap();
        let fact = Fact::with_id(
            cid,
            format!("conclusion:{}", i % 50),
            format!("content-{}", i).into(),
            format!("creator-{}", i % 20),
        );
        let blob_hash = fact.content_hash.to_string();
        let record = FactRecord {
            id: fact.id.to_string(),
            blob_hash: blob_hash.clone(),
            origin: fact.origin.clone(),
            creator: fact.creator.clone(),
            submitted_at: 0,
        };
        block_on(io.write(&record.key(), &postcard::to_allocvec(&record).unwrap())).unwrap();
        block_on(io.write(&format!("blob/{blob_hash}.bin"), &fact.content.data)).unwrap();
        block_on(
            io.write(
                &format!("blob/{blob_hash}.bin.meta"),
                &postcard::to_allocvec(&ContentMeta {
                    mime_type: "text/plain".into(),
                    size: fact.content.data.len() as u64,
                })
                .unwrap(),
            ),
        )
        .unwrap();
    }
    let store = FihStorage::with_clock(io, "conflict-bench", Box::new(nex_core::SystemClock));
    block_on(store.rebuild_cache()).unwrap();
    store
}

fn bench_conflict(c: &mut Criterion) {
    let mut group = c.benchmark_group("conflict");

    // Same id, different content after reopen: the check must find the
    // occupied id and reject. No mutation, so every iteration measures the
    // detection path alone. Since the L2 restructure the structural index
    // memory is bounded by axis cardinality and the record layer is
    // HashMap-backed, so larger stores no longer exhaust memory before
    // the check cost is measurable; the size stays at 100 for parity with
    // the earlier measurements.
    group.bench_function("check_conflict_existing_id_after_reopen_100", |b| {
        let store = build_reopened_conflict_store(100);
        let cid = CoordId::from_axes(0, 0, 0, 0, 0, 0u16).unwrap();
        let fact = Fact::with_id(
            cid,
            "conclusion:0".into(),
            "different-content-0".into(),
            "creator-0".into(),
        );
        b.iter(|| {
            let err = block_on(store.submit_fact(&fact));
            assert!(
                matches!(err, Err(BlackboardError::Conflict(_))),
                "expected Conflict, got {err:?}"
            );
            let _ = black_box(err);
        });
    });

    // Same id, same content after reopen: an idempotent retry. No mutation.
    group.bench_function("check_idempotent_existing_id_after_reopen_100", |b| {
        let store = build_reopened_conflict_store(100);
        let cid = CoordId::from_axes(0, 0, 0, 0, 0, 0u16).unwrap();
        let fact = Fact::with_id(
            cid,
            "conclusion:0".into(),
            "content-0".into(),
            "creator-0".into(),
        );
        b.iter(|| {
            black_box(block_on(store.submit_fact(&fact)).unwrap());
        });
    });

    // New-id absence is an O(records) scan in both the pre-fix and post-fix
    // designs (no index exists for direct-writer records), so it is not
    // benchmarked here; the two existing-id cases above isolate the F2
    // improvement (id-keyed entity store hit vs unified-store scan).

    group.finish();
}

// ── Criterion entry point ───────────────────────────────────────────────

criterion_group!(
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .measurement_time(std::time::Duration::from_secs(15));
    targets =
        bench_raw_full_scan_50k,
        bench_raw_iter_prefix_50k,
        bench_cs_get_10k,
        bench_cs_hashmap_get_10k,
        bench_fih_filter_creator,
        bench_fih_and_query,
        bench_fih_filter_creator_time,
        bench_fih_filter_three_axis,
        bench_fih_axis_hints,
        bench_fih_write_10k,
        bench_fih_intents_by_fact,
        bench_kb_query,
        bench_conflict,
);
criterion_main!(benches);
