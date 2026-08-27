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
//   conflict/ — #176 conflict guard on the occupied-id path
//   multidim/ — #179 real-scenario multi-dimensional search (record-map scan
//               vs structural iter_prefix pruning, 100k and 1M)
//   kb_lifecycle/ — #179 real llms.txt docs + FIH lifecycle simulation
//               (add → query → conclude, at accumulation phases)
//
// Measured on Apple M1 (ARMv8.4-A Firestorm 3.2GHz), release profile
// (median of 10 samples, 2026-08-20). All 19 benches execute in the
// criterion harness. The historical 2026-07-31 figures predate the L2
// restructure and the CoordId<20> migration (issue #176), so only
// within-run comparisons are meaningful. Fresh baseline:
//   raw/full_scan_50k          2.63 s          (50K entries, full tree walk)
//   raw/iter_prefix_50k        1.05 ms         (5×100 prefixes, O(subtree))
//   cs/csn6_get_10k            564 µs          (10K tree lookups, ~56 ns/op)
//   cs/cs2_get_10k             23.7 µs         (10K dense lookups, ~2.4 ns/op)
//   cs/hashmap_get_10k         123 µs          (10K hash lookups, ~12 ns/op)
//   fih/filter_creator_50k     4.93 ms         (structural index + materialize)
//   fih/and_query_50k          1.01 ms         (id-set intersection + materialize)
//   fih/filter_creator_time    475 µs          (creator + time range)
//   fih/filter_three_axis      233 µs          (origin + creator + time)
//   fih/axis_hints_no          681 µs          (no hint, index fallback)
//   fih/axis_hints_with        114 µs          (origin+creator AND; hints not consumed)
//   fih/write_10k_facts        69.3 ms         (batch write + flush)
//   fih/intents_by_fact_100    31.3 µs         (100 calls, inverse index)
//   kb_query/project_author    157 µs          (origin+creator AND)
//   kb_query/project_only      2.27 ms         (single-axis, 1000 hits)
//   kb_query/project_time_range 71.9 µs        (origin+creator+time)
//   kb_query/intents_by_fact   189 µs / 100    (inverse index)
//   conflict/check_conflict    386 ns          (occupied id, guard hit)
//   conflict/check_idempotent  309 ns          (occupied id, idempotent retry)
//
// #179 multidim/ and kb_lifecycle/ (2026-08-26): the real-scenario
// multi-dimensional search. The structural path (structural_fact_ids,
// iter_prefix over the leading axes) vs the record-map scan
// (read_state_filtered) on origin + creator + time-range filters:
//
//   fih/multidim_100k/scan_three_axis_wide    1.44 ms     struct 257 µs   (6x)
//   fih/multidim_100k/scan_three_axis_narrow    940 µs     struct 37.4 µs (25x)
//   fih/multidim_1m/scan_three_axis_wide       25.3 ms     struct 5.00 ms (5x)
//   fih/multidim_1m/scan_three_axis_narrow     18.6 ms     struct 398 µs  (47x)
//   fih/multidim_{100k,1m}/struct_creator_only  slower than scan: without
//     origin fixed, the contiguous-prefix property cannot prune creator
//     (axis order), so the structural path does a full-tree walk; the
//     scan is cheaper (1m: 720 ms vs 110 ms).
//   fih/kb_lifecycle (real llms.txt docs + FIH lifecycle: add → intent →
//     claim → conclude → conclusion fact): 3-axis docs query, struct wins
//     2.4x (phase 1) to 2.9x (phase 3); creator-only, struct 3.4-5.1 ms
//     vs scan 8.5-14 µs (no pruning).
//
// Wiring decision: structural pruning pays for time-bounded
// origin(+creator) filters and the gap widens with scale; it must keep
// the record-map scan as the fallback for filters that cannot form a
// leading-axis prefix (no time bounds, or creator without origin).
//
// The #176 goals dominate the deltas: intents_by_fact went from an O(N)
// scan (743 ms / 100 pre-index) to an O(fan-out) map lookup, and the
// conflict guard is a 386 ns occupied-id check. The read-filter paths
// materialize records from the HashMap record layer through the
// structural index; the historical figures were on the unified tree and
// are not directly comparable.
//
// Paradigm framing: HashMap (full scan) vs Tagma (CoordSpaceN index +
// fast-path tables). The old 28 ms full-scan ceiling is broken by paying
// construction cost only for matches; AND queries shrink the candidate set
// via HashSet intersection at each added dimension.
//
// Scale bound (Phase 3 of #151): the L2 restructure (#176) makes the
// structural filter index memory bounded by axis cardinality, not record
// count. Each CoordSpaceN node is a dense 11,172-slot array, so the number
// of distinct axis combos bounds the tree; record bodies live in HashMap
// record maps. A literal 10M-entry nexus benchmark is therefore not
// meaningful: the same axis-combo space holds 50k or 10M records at the
// same index cost, and the record-layer HashMap is standard hash scaling.
// The tagma primitives themselves are verified at 10M+ scale in the
// syntagma benchmark suite (Sparse get, 10M entries, CS2) referenced by
// issue #151; ev integration (CoordSpaceN as a verification backend) is
// tracked in the ev repository.
// ═══════════════════════════════════════════════════════════════════════════

use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};
use futures_executor::block_on;

// Real SSCCS documentation manifest (generated from docs/_llms/llms.txt):
// (section, area, title) used by the kb_lifecycle scenario.
#[path = "llms_manifest.rs"]
mod llms_manifest;

use nex_fih::{
    AsyncFactCapable, AsyncFilterCapable, AsyncIntentCapable, AsyncStorageRead, AxisHints,
    BlackboardError, ContentMeta, CoordId, Fact, FactRecord, FihStorage, Intent, StateFilter,
};
use nexus_storage_sim::{FileIo, SimIo};

// ── Fixtures: controlled axis distributions ─────────────────────────────

const N_FACTS: usize = 50_000;
const N_ORIGINS: usize = 50;
const N_CREATORS: usize = 20;
const N_INTS: usize = 10_000;

/// Deterministic distinct id for benchmark fixtures (label-derived).
fn bench_id(seed: u64) -> CoordId {
    CoordId::from_label(&format!("bench-{seed}"))
}

/// 10K facts with label-derived ids (real clock timestamps for axis hints).
fn populate_10k(store: &FihStorage<SimIo>) -> u64 {
    let clock = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    for i in 0..10_000u32 {
        let cid = bench_id(i as u64);
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

/// 50K facts with label-derived ids and controlled origin/creator fields.
fn build_fact_store() -> FihStorage<SimIo> {
    let io = SimIo::new();
    let store = FihStorage::with_clock(io, "multi-axis", Box::new(nex_core::SystemClock));

    for i in 0..N_FACTS {
        let cid = bench_id(i as u64);
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
        let cid = bench_id((N_FACTS + i) as u64);
        let intent = Intent::new(
            cid,
            vec![bench_id(i as u64)],
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
        let cid = bench_id(i as u64);
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
        let cid = bench_id(fact_idx as u64);
        let intent_id = bench_id((200_000 + i) as u64);
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

    // The filter implementation does not consume `axis_hints` yet
    // (`StateFilter.axis_hints` is a pending wiring optimization per
    // fih/src/storage/filter.rs), so this pair measures filter narrowing:
    // creator-only vs origin+creator AND. The delta is not a hint-path
    // speedup until the prefix-query wiring lands.
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
            SimIo::new,
            |io| {
                let store = FihStorage::with_clock(io, "write", Box::new(nex_core::SystemClock));
                for i in 0..10_000 {
                    let cid = bench_id(i as u64);
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

// ── multidim/: real-scenario multi-dimensional search (#179) ────────────
//
// Baseline: read_state_filtered scans the application-layer record maps
// with field predicates. Candidate: structural_fact_ids prunes the
// candidate id set with iter_prefix over [time_hi, time_lo, entity,
// origin, creator] and re-applies the exact predicates. Both paths must
// return the identical id set (asserted by benches/tests/structural_search.rs).
//
// The fixture places facts on controlled day buckets (10 days x 10
// origins x 10 creators), so the structural index is identical at every
// scale: tree cost stays constant while the record-map scan grows
// linearly. That constant-index property is the production claim this
// bench measures: an ever-accumulating FIH knowledge network queried
// spatio-temporally (origin + creator + time range) must not degrade
// with record count on local hardware.

const MD_ORIGINS: usize = 10;
const MD_CREATORS: usize = 10;
const MD_DAYS: usize = 10;
const MD_T0_NS: u64 = 1_000_000_000_000_000_000;
const DAY_NS: u64 = 86_400_000_000_000;

/// Clock with a shared handle: the fixture sets the timestamp at each
/// day boundary, so every fact in a day group shares one day bucket.
#[derive(Clone)]
struct StepDayClock(std::sync::Arc<std::sync::Mutex<u64>>);

impl StepDayClock {
    fn new(start: u64) -> Self {
        Self(std::sync::Arc::new(std::sync::Mutex::new(start)))
    }
    fn set(&self, ts: u64) {
        *self.0.lock().unwrap() = ts;
    }
}

impl nex_core::Now for StepDayClock {
    fn now_nanos(&self) -> u64 {
        *self.0.lock().unwrap()
    }
    fn now_secs(&self) -> u64 {
        *self.0.lock().unwrap() / 1_000_000_000
    }
}

/// Fact store with a controlled time distribution: `n_facts` over
/// MD_DAYS day buckets, origin = i % 10, creator = (i / 10) % 10, so
/// each (origin, creator) combo appears n_facts / (MD_DAYS * 100) times
/// per day. Content is deduped to 100 blobs so materialization cost is
/// bounded and scale-independent.
fn build_multidim_store(n_facts: usize, label: &str) -> FihStorage<SimIo> {
    let clock = StepDayClock::new(MD_T0_NS);
    let store = FihStorage::with_clock(SimIo::new(), label, Box::new(clock.clone()));
    let per_day = n_facts / MD_DAYS;
    let mut i = 0usize;
    for day in 0..MD_DAYS {
        clock.set(MD_T0_NS + (day as u64) * DAY_NS);
        for _ in 0..per_day {
            let cid = bench_id(i as u64);
            let fact = Fact::with_id(
                cid,
                format!("origin-{}", i % MD_ORIGINS),
                format!(
                    "Document {}: research content about paradigm shift",
                    i % 100
                )
                .into(),
                format!("creator-{}", (i / MD_ORIGINS) % MD_CREATORS),
            );
            block_on(store.submit_fact(&fact)).unwrap();
            i += 1;
        }
    }
    block_on(store.flush_pending()).unwrap();
    store
}

fn md_since(day: usize) -> String {
    (MD_T0_NS + (day as u64) * DAY_NS).to_string()
}

fn md_until(day: usize) -> String {
    (MD_T0_NS + (day as u64 + 1) * DAY_NS - 1).to_string()
}

fn md_three_axis(start_day: usize, end_day: usize) -> StateFilter {
    StateFilter {
        origin: Some("origin-3".into()),
        creator: Some("creator-3".into()),
        since: Some(md_since(start_day)),
        until: Some(md_until(end_day)),
        ..Default::default()
    }
}

fn md_creator_only(start_day: usize, end_day: usize) -> StateFilter {
    StateFilter {
        creator: Some("creator-3".into()),
        since: Some(md_since(start_day)),
        until: Some(md_until(end_day)),
        ..Default::default()
    }
}

fn bench_multidim(c: &mut Criterion) {
    bench_multidim_scale(c, 100_000, "100k");
    bench_multidim_scale(c, 1_000_000, "1m");
}

fn bench_multidim_scale(c: &mut Criterion, n_facts: usize, label: &str) {
    // One store per scale group: the structural index is identical at
    // both scales (same axis combos), only the record maps grow.
    let store = build_multidim_store(n_facts, label);
    let per_combo_day = n_facts / (MD_DAYS * MD_ORIGINS * MD_CREATORS);
    let wide = md_three_axis(0, MD_DAYS - 1);
    let wide_hits = per_combo_day * MD_DAYS;
    let narrow = md_three_axis(4, 4);
    let narrow_hits = per_combo_day;
    let creator_only = md_creator_only(0, MD_DAYS - 1);
    let creator_only_hits = per_combo_day * MD_DAYS * MD_ORIGINS;

    let mut group = c.benchmark_group(format!("fih/multidim_{label}"));

    group.bench_function("scan_three_axis_wide", |b| {
        b.iter(|| {
            let state = block_on(store.read_state_filtered(black_box(&wide)));
            assert_eq!(state.facts.len(), wide_hits);
            black_box(state);
        });
    });
    group.bench_function("struct_three_axis_wide", |b| {
        b.iter(|| {
            let ids = store.structural_fact_ids(black_box(&wide));
            assert_eq!(ids.len(), wide_hits);
            // Record-map lookups the wired path pays on materialization.
            let recs = store.fact_records.borrow();
            for id in &ids {
                black_box(recs.get(id));
            }
            black_box(ids);
        });
    });
    group.bench_function("scan_three_axis_narrow", |b| {
        b.iter(|| {
            let state = block_on(store.read_state_filtered(black_box(&narrow)));
            assert_eq!(state.facts.len(), narrow_hits);
            black_box(state);
        });
    });
    group.bench_function("struct_three_axis_narrow", |b| {
        b.iter(|| {
            let ids = store.structural_fact_ids(black_box(&narrow));
            assert_eq!(ids.len(), narrow_hits);
            let recs = store.fact_records.borrow();
            for id in &ids {
                black_box(recs.get(id));
            }
            black_box(ids);
        });
    });
    // Creator-only: origin is not fixed, so the contiguous-prefix
    // property of iter_prefix cannot prune creator; the exact predicate
    // carries the selectivity. Measures the axis-order limitation.
    group.bench_function("scan_creator_only_wide", |b| {
        b.iter(|| {
            let state = block_on(store.read_state_filtered(black_box(&creator_only)));
            assert_eq!(state.facts.len(), creator_only_hits);
            black_box(state);
        });
    });
    group.bench_function("struct_creator_only_wide", |b| {
        b.iter(|| {
            let ids = store.structural_fact_ids(black_box(&creator_only));
            assert_eq!(ids.len(), creator_only_hits);
            let recs = store.fact_records.borrow();
            for id in &ids {
                black_box(recs.get(id));
            }
            black_box(ids);
        });
    });

    group.finish();
}

// ── kb_lifecycle/: real-document FIH lifecycle simulation (#179) ────────
//
// Ingests the real docs.ssccs.org/llms.txt manifest (embedded in
// llms_manifest.rs), then simulates the FIH lifecycle per document:
// fact (document) → intent (analyze) → claim → conclude → conclusion
// fact (new knowledge, origin "conclusion:<intent>", creator = worker).
// Replications advance the clock by one day, so the accumulated
// knowledge network grows in time. Queries at accumulation phases
// compare the record-map scan against the structural iter_prefix path
// on the real section/area/time axes.

const LC_REPS: usize = 3;

fn build_lifecycle_store() -> FihStorage<SimIo> {
    let clock = StepDayClock::new(MD_T0_NS);
    let store = FihStorage::with_clock(SimIo::new(), "kb-lifecycle", Box::new(clock.clone()));
    for rep in 0..LC_REPS {
        clock.set(MD_T0_NS + (rep as u64) * DAY_NS);
        for (i, (section, area, title)) in llms_manifest::LLMS_DOCS.iter().enumerate() {
            let doc_cid = CoordId::from_label(&format!("doc-{rep}-{i}"));
            let doc = Fact::with_id(
                doc_cid,
                (*section).into(),
                format!("{title}\n\nMarkdown body of the {title} document.").into(),
                (*area).into(),
            );
            block_on(store.submit_fact(&doc)).unwrap();
            let intent_id = format!("analyze-{rep}-{i}");
            let intent = Intent::new(
                CoordId::from_label(&intent_id),
                vec![doc_cid],
                None,
                format!("analyze {title}"),
                (*area).into(),
            );
            block_on(store.submit_intent(&intent)).unwrap();
            block_on(store.claim_intent(&intent_id, area)).unwrap();
            let conclusion = format!("conclusion for {title}");
            block_on(store.conclude_intent(&intent_id, &conclusion)).unwrap();
        }
    }
    block_on(store.flush_pending()).unwrap();
    store
}

/// Number of manifest docs matching the section/area pair (None matches
/// any value on that axis).
fn lc_count(section: Option<&str>, area: Option<&str>) -> usize {
    llms_manifest::LLMS_DOCS
        .iter()
        .filter(|(s, a, _)| section.is_none_or(|x| *s == x) && area.is_none_or(|x| *a == x))
        .count()
}

fn bench_kb_lifecycle(c: &mut Criterion) {
    let store = build_lifecycle_store();
    let mut group = c.benchmark_group("fih/kb_lifecycle");

    // Query shapes on the real manifest axes. Phase p covers days 0..p.
    for phase in [1usize, LC_REPS] {
        let since = md_since(0);
        let until = md_until(phase - 1);
        // Section + area + time: the projects/nexus docs by the nexus
        // maintainer (doc facts only; conclusion origins differ).
        let f1 = StateFilter {
            origin: Some("projects".into()),
            creator: Some("nexus".into()),
            since: Some(since.clone()),
            until: Some(until.clone()),
            ..Default::default()
        };
        let hits1 = lc_count(Some("projects"), Some("nexus")) * phase;
        // Creator-only + time: the notes-area maintainer, including the
        // conclusion facts they produced (one per doc, worker = area).
        let f2 = StateFilter {
            creator: Some("notes".into()),
            since: Some(since),
            until: Some(until),
            ..Default::default()
        };
        let hits2 = lc_count(None, Some("notes")) * phase * 2;

        group.bench_function(format!("scan_nexus_docs_phase{phase}"), |b| {
            b.iter(|| {
                let s = block_on(store.read_state_filtered(black_box(&f1)));
                assert_eq!(s.facts.len(), hits1);
                black_box(s);
            });
        });
        group.bench_function(format!("struct_nexus_docs_phase{phase}"), |b| {
            b.iter(|| {
                let ids = store.structural_fact_ids(black_box(&f1));
                assert_eq!(ids.len(), hits1);
                black_box(ids);
            });
        });
        group.bench_function(format!("scan_notes_creator_phase{phase}"), |b| {
            b.iter(|| {
                let s = block_on(store.read_state_filtered(black_box(&f2)));
                assert_eq!(s.facts.len(), hits2);
                black_box(s);
            });
        });
        group.bench_function(format!("struct_notes_creator_phase{phase}"), |b| {
            b.iter(|| {
                let ids = store.structural_fact_ids(black_box(&f2));
                assert_eq!(ids.len(), hits2);
                black_box(ids);
            });
        });
    }
    group.finish();
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
                let cid = bench_id(fidx as u64);
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
/// `rebuild_cache` loads them: the in-memory record map is empty and
/// the record maps plus the structural filter index are populated
/// (the id-keyed entity store was merged into the record maps).
fn build_reopened_conflict_store(n: usize) -> FihStorage<SimIo> {
    let io = SimIo::new();
    for i in 0..n {
        let cid = bench_id(i as u64);
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
        let cid = bench_id(0);
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
        let cid = bench_id(0);
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

    // New-id absence is an O(1) record-map miss since the L2 restructure
    // (`existing_fact_content_hash` reads the id-keyed `fact_records`
    // map, which direct writers also populate via `place_record`), so it
    // is not benchmarked here; the two existing-id cases above isolate
    // the guard cost on the occupied-id path.

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
        bench_multidim,
        bench_kb_lifecycle,
);
criterion_main!(benches);
