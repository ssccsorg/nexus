// The by-id filter on both filter paths.
//
// This scenario has a target of its own because of how a criterion target runs: every fixture in
// it is built before the filter can skip a benchmark, so a group measured from the `bench`
// target pays for the million-fact multi-dimensional fixture first, which costs more than this
// measurement is worth. Here the volume is ten thousand records and the target runs in seconds.

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use futures_executor::block_on;
use nex_fih::{
    AsyncFactCapable, AsyncFilterCapable, Content, CoordId, Fact, FihStorage, StateFilter,
};
use nexus_storage_sim::SimIo;

const FACTS: usize = 10_000;
const WANTED: usize = 100;

fn id_of(tag: &str) -> CoordId {
    CoordId::from_label(tag)
}

/// A volume whose ids are derived from a label, so the wanted list can name records without
/// reading the volume first.
///
/// The volume is flushed before it is measured: `read_state_filtered` builds its blob lookup
/// from the session's unflushed buffer, which at this volume is a cost of the session rather
/// than of the filter, and a query over ten thousand records is a query over a volume already on
/// the medium.
fn store_of(facts: usize) -> FihStorage<SimIo> {
    let store = FihStorage::new(SimIo::new(), "wanted-ids");
    for i in 0..facts {
        let fact = Fact::with_id(
            id_of(&format!("wanted-{i}")),
            format!("origin-{}", i % 10),
            Content::from(format!("payload {i}").as_str()),
            format!("creator-{}", i % 10),
        );
        block_on(store.submit_fact(&fact)).expect("the fact is accepted");
    }
    block_on(store.flush_pending()).expect("the writes reach the medium");
    store
}

/// One hundred wanted ids over a volume of ten thousand records, with no other predicate, so
/// every record is a candidate and the membership test is the whole cost of the filter.
fn bench_wanted_ids(c: &mut Criterion) {
    let store = store_of(FACTS);
    let wanted: Vec<String> = (0..WANTED)
        .map(|i| id_of(&format!("wanted-{i}")).to_string())
        .collect();
    let filter = StateFilter {
        fact_ids: Some(wanted),
        ..Default::default()
    };

    let mut group = c.benchmark_group("fih/wanted_ids");
    group.bench_function("scan", |b| {
        b.iter(|| {
            let state = block_on(store.read_state_filtered(black_box(&filter)));
            assert_eq!(state.facts.len(), WANTED);
            black_box(state);
        });
    });
    group.bench_function("struct", |b| {
        b.iter(|| {
            let ids = store.structural_fact_ids(black_box(&filter));
            assert_eq!(ids.len(), WANTED);
            black_box(ids);
        });
    });
    group.finish();
}

criterion_group!(benches, bench_wanted_ids);
criterion_main!(benches);
