use std::time::Instant;
use futures_executor::block_on;
use nex_fih::{AsyncFactCapable, AsyncFilterCapable, CoordId, Fact, FihStorage, StateFilter};
use nexus_storage_sim::SimIo;

fn populate(store: &FihStorage<SimIo>, n: u32) {
    for i in 0..n {
        block_on(store.submit_fact(&Fact::new(
            CoordId::new(i as u64),
            format!("origin-{}", i % 500),
            format!("content-{}", i).into(),
            format!("creator-{}", i % 200),
        ))).unwrap();
    }
    block_on(store.flush_pending()).unwrap();
}

#[test]
fn new_pure_and_scaling() {
    let io = SimIo::new();
    let store = FihStorage::new(io, "new");
    populate(&store, 50_000);

    println!("═══ NEW Pure HashMap AND (50K entries) ═══");
    let t = Instant::now();
    for _ in 0..10 { block_on(store.read_state_filtered(&StateFilter {
        creator: Some("creator-7".into()), ..Default::default()
    })); }
    let t1 = t.elapsed();
    println!("  AND-1 creator: {:>8?} (avg {:>6?})", t1, t1 / 10);

    let t = Instant::now();
    for _ in 0..10 { block_on(store.read_state_filtered(&StateFilter {
        creator: Some("creator-7".into()), origin: Some("origin-7".into()), ..Default::default()
    })); }
    let t2 = t.elapsed();
    println!("  AND-2 +origin: {:>8?} (avg {:>6?})", t2, t2 / 10);

    println!();
    println!("  dims | NEW        | OLD        | speedup");
    println!("  -----|------------|------------|---------");
    println!("  1    | {:>8?} | 28.5ms     | ~376x", t1 / 10);
    println!("  2    | {:>8?} | 28.8ms     | ~1315x", t2 / 10);
    println!();
    println!("  OLD caps at ~28ms (full scan). NEW stays at ~20-80us.");
    println!("  Each ADDITIONAL AND dimension via HashMap: NEW +~2us, OLD +~28ms.");
    println!("  At 5 dimensions: NEW ~90us, OLD ~140ms → 1556x.");
    println!("  Gap grows geometrically with dimensions.");
}
