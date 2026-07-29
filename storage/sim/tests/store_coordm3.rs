// CoordSpaceM3: mmap-backed dense (N=3, virtual 1.27TB, MAP_NORESERVE)
// Run: cargo test --release -p nexus-storage-sim --test store_coordm3 -- --nocapture

use std::time::Instant;
use tagma_core::{Coord, CoordPath, CoordSpaceM3, CoordSpaceN};

fn pack_6to3(axes: &[u16; 6]) -> CoordPath<3> {
    let packed = (((axes[2] as u64 * 11172 + axes[3] as u64) * 11172 + axes[4] as u64) * 11172 + axes[5] as u64) % 11172;
    CoordPath::new([
        Coord::new(axes[0]).unwrap(),
        Coord::new(axes[1]).unwrap(),
        Coord::new(packed as u16).unwrap(),
    ])
}

fn gen_axes(i: u32) -> [u16; 6] {
    let clock = 1700000000000000u64;
    [
        ((clock + i as u64) / 86_400_000_000_000 % 11172) as u16,
        ((clock + i as u64) % 86_400_000_000_000 % 11172) as u16,
        0u16,
        (i % 500) as u16,
        (i % 200) as u16,
        (i / 1000) as u16,
    ]
}

#[test]
fn bench_csm3() {
    let n = 10_000u32;
    let axes: Vec<[u16; 6]> = (0..n).map(gen_axes).collect();

    // CoordSpaceN6 (tree, baseline)
    let mut csn: CoordSpaceN<6, u64> = CoordSpaceN::new();
    for (i, a) in axes.iter().enumerate() {
        let c = [Coord::new(a[0]).unwrap(), Coord::new(a[1]).unwrap(),
            Coord::new(a[2]).unwrap(), Coord::new(a[3]).unwrap(),
            Coord::new(a[4]).unwrap(), Coord::new(a[5]).unwrap()];
        csn.place_path(&CoordPath::new(c), i as u64);
    }
    let t = Instant::now();
    for (i, a) in axes.iter().enumerate() {
        let c = [Coord::new(a[0]).unwrap(), Coord::new(a[1]).unwrap(),
            Coord::new(a[2]).unwrap(), Coord::new(a[3]).unwrap(),
            Coord::new(a[4]).unwrap(), Coord::new(a[5]).unwrap()];
        let _ = csn.at_path(&CoordPath::new(c));
    }
    let read_n6 = t.elapsed();

    // CoordSpaceM3 (mmap dense)
    let mut csm3: CoordSpaceM3<u64> = CoordSpaceM3::new();
    for (i, a) in axes.iter().enumerate() {
        csm3.place_path(&pack_6to3(a), i as u64);
    }
    let t = Instant::now();
    for (i, a) in axes.iter().enumerate() {
        let _ = csm3.at_path(&pack_6to3(a));
    }
    let read_m3 = t.elapsed();

    println!("═══ CoordSpaceM3 vs CoordSpaceN6 (10K entries, u64) ═══");
    println!();
    println!("  Type            | at_path 10K  | per-op");
    println!("  ----------------|--------------|--------");
    println!("  CoordSpaceN6    | {:>10?}  | {:.0} ns", read_n6, read_n6.as_nanos() as f64 / n as f64);
    println!("  CoordSpaceM3    | {:>10?}  | {:.0} ns", read_m3, read_m3.as_nanos() as f64 / n as f64);
    let ratio = read_n6.as_nanos() as f64 / read_m3.as_nanos().max(1) as f64;
    println!();
    println!("  CoordSpaceM3 at_path: {:.0}x faster than CoordSpaceN6", ratio);
    println!("  (mmap dense: 0.40ns per tagma benchmark; 11172^3 = 1.4T virtual)");
}
