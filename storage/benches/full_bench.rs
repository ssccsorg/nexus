// NEW system benchmark (ssccs-nexus2 161 branch, CoordSpaceN<6> + axis_hints)
// Run: cargo test --release -p nexus-storage-sim --test full_bench -- --nocapture

use std::time::Instant;
use futures_executor::block_on;
use nex_fih::{AsyncFactCapable, AsyncFilterCapable, AsyncStorageRead, AxisHints, CoordId, Fact, FihStorage, StateFilter};
use nexus_storage_sim::SimIo;

fn populate(store: &FihStorage<SimIo>, n: u32) -> u64 {
    let clock = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64;
    for i in 0..n {
        let time_hi = ((clock + i as u64) / 86_400_000_000_000 % 11172) as u16;
        let time_lo = ((clock + i as u64) % 86_400_000_000_000 % 11172) as u16;
        let origin = (i % 50) as u16;
        let creator = (i % 20) as u16;
        let serial = (i / 1000) as u16;
        let id = CoordId::from_axes(time_hi, time_lo, 0, origin, creator, serial).unwrap();
        let fact = Fact::new(id, format!("origin-{}", i % 50), format!("content-{}", i).into(), format!("creator-{}", i % 20));
        block_on(store.submit_fact(&fact)).unwrap();
    }
    block_on(store.flush_pending()).unwrap();
    clock
}

#[test]
fn new_full_bench() {
    // ── WRITE ──
    let io = SimIo::new();
    let store = FihStorage::new(io, "newbench");
    let t = Instant::now();
    let clock = populate(&store, 10_000);
    println!("[NEW] write 10K: {:?}", t.elapsed());

    // ── READ STATE ──
    let t = Instant::now();
    let state = block_on(store.read_state());
    println!("[NEW] read_state: {:?} ({} facts)", t.elapsed(), state.facts.len());

    // ── SINGLE FIELD FILTER (no hints, full scan) ──
    let t = Instant::now();
    for _ in 0..10 {
        let s = block_on(store.read_state_filtered(&StateFilter {
            creator: Some("creator-5".into()), ..Default::default()
        }));
        assert_eq!(s.facts.len(), 500);
    }
    println!("[NEW] filter creator 10x (no hints): {:?} (avg {:?})", t.elapsed(), t.elapsed() / 10);

    // ── AND QUERY with axis_hints (origin=7 + creator=7) ──
    // Find correct time values from actual data
    let mut target_th = 0u16;
    let mut target_tl = 0u16;
    for i in 0..10_000u32 {
        if i % 100 == 7 { // lcm(50,20)=100, so i%100==7 gives i%50==7 AND i%20==7
            target_th = ((clock + i as u64) / 86_400_000_000_000 % 11172) as u16;
            target_tl = ((clock + i as u64) % 86_400_000_000_000 % 11172) as u16;
            break;
        }
    }

    let t = Instant::now();
    for _ in 0..10 {
        let s = block_on(store.read_state_filtered(&StateFilter {
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
        assert!(s.facts.len() >= 1, "should find origin=7 AND creator=7 with hints");
    }
    println!("[NEW] AND query with hints 10x: {:?} (avg {:?})", t.elapsed(), t.elapsed() / 10);

    // ── AND QUERY without hints (full scan comparison) ──
    let t = Instant::now();
    for _ in 0..10 {
        let s = block_on(store.read_state_filtered(&StateFilter {
            origin: Some("origin-5".into()),
            creator: Some("creator-5".into()),
            ..Default::default()
        }));
        assert_eq!(s.facts.len(), 100);
    }
    println!("[NEW] AND query no hints 10x: {:?} (avg {:?})", t.elapsed(), t.elapsed() / 10);
}
