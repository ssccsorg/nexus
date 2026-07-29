// Comparison: Unified CoordSpaceN<19> + CoordId system.
//
// Run: cargo test -p nexus-storage-sim -- comparison --nocapture

use futures_executor::block_on;
use nex_fih::{
    AsyncFactCapable, AsyncFilterCapable, AsyncStorageRead, CoordId, Fact, FihStorage, StateFilter,
};
use nexus_storage_sim::SimIo;

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
        let fact = Fact::new(
            cid,
            format!("origin-{}", i % 50),
            format!("content-{}", i).into(),
            format!("creator-{}", i % 20),
        );
        block_on(store.submit_fact(&fact)).unwrap();
    }
    block_on(store.flush_pending()).unwrap();
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
        let state = block_on(store.read_state_filtered(&StateFilter {
            creator: Some("creator-5".into()),
            ..Default::default()
        }));
        assert_eq!(state.facts.len(), 500);
    }
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
}
