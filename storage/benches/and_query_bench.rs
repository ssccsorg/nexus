// AND query benchmark: CoordSpaceN iter_prefix for true index usage.
//
// Run: cargo test --release -p nexus-storage-sim --test and_query_bench -- --nocapture

use std::time::Instant;

#[test]
fn and_query_raw() {
    use tagma_core::{Coord, CoordPath, CoordSpaceN};

    let mut space: CoordSpaceN<6, u32> = CoordSpaceN::new();

    for i in 0..50_000u32 {
        let time_hi = (i / 10000) as u16;
        let time_lo = ((i / 100) % 100) as u16;
        let origin = (i % 200) as u16;
        let creator = (i % 100) as u16;
        let serial = (i % 1000) as u16;
        let coords = [
            Coord::new(time_hi).unwrap(),
            Coord::new(time_lo).unwrap(),
            Coord::new(0).unwrap(),
            Coord::new(origin).unwrap(),
            Coord::new(creator).unwrap(),
            Coord::new(serial).unwrap(),
        ];
        space.place_path(&CoordPath::new(coords), i);
    }
    assert_eq!(space.len(), 50000);

    let t = Instant::now();
    for (_path, _v) in space.iter_tree() {}
    let full_scan = t.elapsed();

    let t = Instant::now();
    let mut n = 0u32;
    for (_path, _v) in space.iter_tree() {
        if _path.coords()[3].index() == 7 && _path.coords()[4].index() == 7 {
            n += 1;
        }
    }
    let scan_and = t.elapsed();
    assert_eq!(n, 250);

    let t = Instant::now();
    n = 0;
    for th in 0..5u16 {
        for tl in 0..100u16 {
            let prefix = [
                Coord::new(th).unwrap(),
                Coord::new(tl).unwrap(),
                Coord::new(0).unwrap(),
                Coord::new(7).unwrap(),
                Coord::new(7).unwrap(),
            ];
            if let Some(iter) = space.iter_prefix(&prefix) {
                for (_p, _v) in iter {
                    n += 1;
                }
            }
        }
    }
    let prefix_and = t.elapsed();
    assert_eq!(n, 250);

    let t = Instant::now();
    n = 0;
    if let Some(iter) = space.iter_prefix(&[Coord::new(2).unwrap()]) {
        for (_p, _v) in iter {
            n += 1;
        }
    }
    let prefix_time = t.elapsed();
    assert_eq!(n, 10000);

    let t = Instant::now();
    n = 0;
    let prefix = [Coord::new(2).unwrap(), Coord::new(50).unwrap(), Coord::new(0).unwrap()];
    if let Some(iter) = space.iter_prefix(&prefix) {
        for (_p, _v) in iter {
            n += 1;
        }
    }
    let prefix_3 = t.elapsed();
    assert_eq!(n, 100);

    println!("═══ CoordSpaceN<6> AND Query (50K entries) ═══");
    println!();
    println!("  Operation                       | time         | vs full scan");
    println!("  --------------------------------|--------------|------------");
    println!("  full scan 50K                    | {:>8?}   | 1.0x", full_scan);
    println!("  AND origin=7+creator=7 (fullscan)| {:>8?}   | {:.1}x",
        scan_and, scan_and.as_nanos() as f64 / full_scan.as_nanos() as f64);
    println!("  AND origin=7+creator=7 (prefix)  | {:>8?}   | {:.0}x faster",
        prefix_and, full_scan.as_nanos() as f64 / prefix_and.as_nanos().max(1) as f64);
    println!("  prefix time_hi=2                 | {:>8?}   | {:.0}x faster",
        prefix_time, full_scan.as_nanos() as f64 / prefix_time.as_nanos().max(1) as f64);
    println!("  prefix 3-axis                   | {:>8?}   | {:.0}x faster",
        prefix_3, full_scan.as_nanos() as f64 / prefix_3.as_nanos().max(1) as f64);
}
