// Comparison: new CoordSpaceN + CoordId system.
//
// Run: cargo test -p nexus-storage-sim -- comparison --nocapture

use futures_executor::block_on;
use nex_fih::{
    AsyncFactCapable, AsyncFilterCapable, AsyncStorageRead, CoordId, Fact, FihStorage, StateFilter,
};
use nexus_storage_sim::SimIo;

#[test]
fn comparison_write_10k() {
    let io = SimIo::new();
    let store = FihStorage::new(io, "compare");

    let start = std::time::Instant::now();
    for i in 0..10_000 {
        let fact = Fact::new(
            CoordId::new(i),
            format!("origin-{}", i % 50),
            format!("content-{}", i).into(),
            format!("creator-{}", i % 20),
        );
        block_on(store.submit_fact(&fact)).unwrap();
    }
    block_on(store.flush_pending()).unwrap();
    let elapsed = start.elapsed();
    println!("[NEW] write 10K facts: {:?}", elapsed);
}

#[test]
fn comparison_read_state() {
    let io = SimIo::new();
    let store = FihStorage::new(io, "compare");

    for i in 0..10_000 {
        let fact = Fact::new(
            CoordId::new(i),
            format!("origin-{}", i % 50),
            format!("content-{}", i).into(),
            format!("creator-{}", i % 20),
        );
        block_on(store.submit_fact(&fact)).unwrap();
    }
    block_on(store.flush_pending()).unwrap();

    let start = std::time::Instant::now();
    let state = block_on(store.read_state());
    let elapsed = start.elapsed();
    println!("[NEW] read_state ({} facts): {:?}", state.facts.len(), elapsed);
}

#[test]
fn comparison_filter_by_creator() {
    let io = SimIo::new();
    let store = FihStorage::new(io, "compare");

    for i in 0..10_000 {
        let fact = Fact::new(
            CoordId::new(i),
            format!("origin-{}", i % 50),
            format!("content-{}", i).into(),
            format!("creator-{}", i % 20),
        );
        block_on(store.submit_fact(&fact)).unwrap();
    }
    block_on(store.flush_pending()).unwrap();

    let start = std::time::Instant::now();
    for _ in 0..10 {
        let state = block_on(store.read_state_filtered(&StateFilter {
            creator: Some("creator-5".into()),
            ..Default::default()
        }));
        assert_eq!(state.facts.len(), 500);
    }
    let elapsed = start.elapsed();
    println!("[NEW] filter by creator (10x): {:?} (avg {:?})", elapsed, elapsed / 10);
}
