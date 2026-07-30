// Axis hint benchmark: with hints (iter_prefix) vs without (full scan).
//
// Run: cargo test --release -p nexus-storage-sim --test axis_bench -- --nocapture

use std::time::Instant;
use futures_executor::block_on;
use nex_fih::{AsyncFactCapable, AsyncFilterCapable, AsyncStorageRead, CoordId, Fact, FihStorage, StateFilter, AxisHints};
use nexus_storage_sim::SimIo;

fn populate_with_clock(store: &FihStorage<SimIo>, n: u32) -> u64 {
    let clock = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64;
    for i in 0..n {
        let time_hi = ((clock + i as u64) / 86_400_000_000_000 % 11172) as u16;
        let time_lo = ((clock + i as u64) % 86_400_000_000_000 % 11172) as u16;
        let origin = (i % 50) as u16;
        let creator = (i % 20) as u16;
        let serial = (i / 1000) as u16;
        let id = CoordId::from_axes(time_hi, time_lo, 0, origin, creator, serial).unwrap();
        let fact = Fact::new(
            id,
            format!("origin-{}", i % 50),
            format!("content-{}", i).into(),
            format!("creator-{}", i % 20),
        );
        block_on(store.submit_fact(&fact)).unwrap();
    }
    block_on(store.flush_pending()).unwrap();
    clock
}

#[test]
fn bench_with_hints_vs_without() {
    // WITHOUT hints: full scan + string filter
    let io = SimIo::new();
    let store = FihStorage::new(io, "no_hints");
    populate_with_clock(&store, 10_000);

    let t = Instant::now();
    let state = block_on(store.read_state_filtered(&StateFilter {
        creator: Some("creator-5".into()),
        ..Default::default()
    }));
    let no_hints = t.elapsed();
    println!("[NO-HINTS] filter creator=5: {:?} ({} facts)", no_hints, state.facts.len());
    assert_eq!(state.facts.len(), 500);

    // WITH hints: same clock, same axis values → iter_prefix
    let io = SimIo::new();
    let store = FihStorage::new(io, "with_hints");
    let clock = populate_with_clock(&store, 10_000);

    // Find the FIRST entry with origin=7 and creator=7 to get correct time values.
    let mut target_th = 0u16;
    let mut target_tl = 0u16;
    for i in 0..10_000u32 {
        if i % 50 == 7 && i % 20 == 7 {
            let th2 = ((clock + i as u64) / 86_400_000_000_000 % 11172) as u16;
            let tl2 = ((clock + i as u64) % 86_400_000_000_000 % 11172) as u16;
            target_th = th2;
            target_tl = tl2;
            break;
        }
    }

    // Query with CORRECT axis values → iter_prefix finds entries
    let t = Instant::now();
    let state = block_on(store.read_state_filtered(&StateFilter {
        origin: Some("origin-7".into()),
        creator: Some("creator-7".into()),
        axis_hints: Some(AxisHints {
            time_hi: Some(target_th),
            time_lo: Some(target_tl),
            entity: Some(0),
            origin: Some(7),
            creator: Some(7),
            serial: None,
        }),
        ..Default::default()
    }));
    let with_hints = t.elapsed();
    println!("[WITH-HINTS] origin=7 + creator=7: {:?} ({} facts)", with_hints, state.facts.len());
    assert!(state.facts.len() > 0, "should find entries with origin=7, creator=7 using correct hints");

    // Comparison: WITHOUT hints for same AND query
    let io = SimIo::new();
    let store = FihStorage::new(io, "no_hints2");
    populate_with_clock(&store, 10_000);
    let t = Instant::now();
    let state2 = block_on(store.read_state_filtered(&StateFilter {
        origin: Some("origin-7".into()),
        creator: Some("creator-7".into()),
        ..Default::default()
    }));
    let no_hints2 = t.elapsed();
    println!("[NO-HINTS] same AND query: {:?} ({} facts)", no_hints2, state2.facts.len());

    println!();
    println!("═══ Summary ═══");
    println!("  filter creator=5 (no hints): {:?}", no_hints);
    println!("  AND query (no hints):        {:?}", no_hints2);
    println!("  AND query (with hints):      {:?}", with_hints);
    if no_hints2 > with_hints && with_hints.as_nanos() > 0 {
        println!("  speedup: {:.0}x", no_hints2.as_nanos() as f64 / with_hints.as_nanos() as f64);
    }
}
