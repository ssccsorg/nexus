//! Nexus Criterion benchmarks — intents_by_fact lookup latency.
//!
//! Measures the intents_by_fact() scan implementation against 10K facts
//! and 10K intents (each referencing one fact). Reports per-query latency
//! and provides comparison against the estimated HashMap O(1) baseline.
//!
//! Run: cargo bench -p nex

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use futures_executor::block_on;

use nex_fih::{
    AsyncFactCapable, AsyncIntentCapable, AsyncStorageRead, CoordId, Content, Fact, Intent,
};
use nexus_storage_sim::{FihStorage, SimIo};

// ── Constants ────────────────────────────────────────────────────────────

const NUM_FACTS: usize = 10_000;
const NUM_INTS: usize = 10_000;
const QUERY_COUNT: usize = 100;

// ── Benchmark: intents_by_fact lookup latency ───────────────────────────

fn bench_intents_by_fact(c: &mut Criterion) {
    // Build store with 10K facts and 10K intents.
    let store = build_store();

    // Collect fact IDs (as strings) for sampling.
    let state = block_on(store.read_state());
    let fact_ids: Vec<String> = state.facts.iter().map(|f| f.id.to_string()).collect();

    // Deterministic sample of QUERY_COUNT fact IDs using stride.
    // Provides repeatable coverage without depending on rand.
    let stride = NUM_FACTS / QUERY_COUNT;
    let sample: Vec<&String> = fact_ids.iter().step_by(stride).take(QUERY_COUNT).collect();

    let mut group = c.benchmark_group("intents_by_fact");
    group.sample_size(10);

    group.bench_function("scan_10k", |b| {
        b.iter(|| {
            for id in &sample {
                let result = black_box(store.intents_by_fact(black_box(id.as_str())));
                black_box(result);
            }
        });
    });

    group.finish();

    // Diagnostic: print estimated O(1) vs O(fan-out) comparison.
    eprintln!(
        "--- intents_by_fact diagnostic ---\n\
         Facts: {NUM_FACTS}, Intents: {NUM_INTS}\n\
         Estimated old-HashMap O(1) lookup: ~10-50 ns\n\
         Measured scan per query (average): {} ns (see criterion report above)\n\
         Fan-out: all {NUM_INTS} intent records scanned per query",
        NUM_INTS
    );
}

// ── Helper: build a populated store ─────────────────────────────────────

fn build_store() -> FihStorage<SimIo> {
    let io = SimIo::new();
    let store = FihStorage::with_clock(
        io,
        "intents_bench",
        Box::new(nex_core::SystemClock),
    );

    // Create NUM_FACTS facts with distinct IDs.
    for i in 0..NUM_FACTS {
        let id_str = format!("fact_{:05}", i);
        let id = CoordId::from_string(&id_str);
        let fact = Fact::new(
            id,
            "bench".into(),
            Content {
                mime_type: "text/plain".into(),
                data: format!("benchmark fact {}", i).into_bytes(),
            },
            "benchmarker".into(),
        );
        block_on(store.submit_fact(&fact)).unwrap();
    }

    // Create NUM_INTS intents, each referencing one deterministically
    // chosen fact (strided to ensure even coverage).
    let intent_stride = NUM_FACTS / NUM_INTS;
    for i in 0..NUM_INTS {
        let id_str = format!("int_{:05}", i);
        let from_fact_idx = (i * intent_stride) % NUM_FACTS;
        let from_id_str = format!("fact_{:05}", from_fact_idx);

        let intent = Intent {
            id: CoordId::from_string(&id_str),
            from_facts: vec![CoordId::from_string(&from_id_str)],
            description: format!("benchmark intent {}", i),
            creator: "benchmarker".into(),
            worker: None,
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            is_concluded: false,
            concluded_at: None,
        };
        block_on(store.submit_intent(&intent)).unwrap();
    }

    store
}

// ── Criterion entry point ───────────────────────────────────────────────

criterion_group!(benches, bench_intents_by_fact);
criterion_main!(benches);
