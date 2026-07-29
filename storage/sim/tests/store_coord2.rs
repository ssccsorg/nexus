// CoordSpace2: dense heap-allocated (N=2, 11172^2 = 125M slots)
// vs CoordSpaceN6: sparse tree (N=6, current default)
// Run: cargo test --release -p nexus-storage-sim --test store_coord2 -- --nocapture

use std::time::Instant;
use tagma_core::{Coord, CoordPath, CoordSpace2, CoordSpaceN};

// Pack 6 FIH axes into 2 CoordSpace2 Coords
// axis[0]=time_hi, [1]=time_lo → Coord0 = time_hi * 11172 + time_lo
// axis[2]=entity, [3]=origin, [4]=creator, [5]=serial → Coord1 = ((entity*11172 + origin)*11172 + creator)*11172 + serial
fn pack_6to2(axes: &[u16; 6]) -> CoordPath<2> {
    let c0 = (axes[0] as u64 * 11172 + axes[1] as u64) % 11172;
    let c1 = ((((axes[2] as u64 * 11172 + axes[3] as u64) * 11172 + axes[4] as u64) * 11172 + axes[5] as u64) % 11172);
    CoordPath::new([
        Coord::new(c0 as u16).unwrap(),
        Coord::new(c1 as u16).unwrap(),
    ])
}

// Pack 6 axes into 3 CoordSpaceM3 Coords
fn pack_6to3(axes: &[u16; 6]) -> CoordPath<3> {
    let c0 = axes[0];
    let c1 = axes[1];
    let c2 = ((((axes[2] as u64 * 11172 + axes[3] as u64) * 11172 + axes[4] as u64) * 11172 + axes[5] as u64) % 11172);
    CoordPath::new([
        Coord::new(c0).unwrap(),
        Coord::new(c1).unwrap(),
        Coord::new(c2 as u16).unwrap(),
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
fn bench_store_types() {
    let n = 10_000u32;
    let axes: Vec<[u16; 6]> = (0..n).map(gen_axes).collect();

    // CoordSpaceN<6, u64> (current tree)
    let t = Instant::now();
    let mut csn: CoordSpaceN<6, u64> = CoordSpaceN::new();
    for (i, a) in axes.iter().enumerate() {
        let coords = [
            Coord::new(a[0]).unwrap(), Coord::new(a[1]).unwrap(),
            Coord::new(a[2]).unwrap(), Coord::new(a[3]).unwrap(),
            Coord::new(a[4]).unwrap(), Coord::new(a[5]).unwrap(),
        ];
        csn.place_path(&CoordPath::new(coords), i as u64);
    }
    let write_csn = t.elapsed();

    // CoordSpace2<u64> (dense heap)
    let t = Instant::now();
    let mut cs2: CoordSpace2<u64> = CoordSpace2::new();
    for (i, a) in axes.iter().enumerate() {
        let path = pack_6to2(a);
        cs2.place_path(&path, i as u64);
    }
    let write_cs2 = t.elapsed();

    println!("═══ CoordSpace Storage Type Comparison (10K entries, u64) ═══");
    println!();
    println!("  Type            | Write 10K | at_path 10K | iter_tree");
    println!("  ----------------|-----------|-------------|----------");

    // Read: at_path all entries
    let t = Instant::now();
    for (i, a) in axes.iter().enumerate() {
        let coords = [Coord::new(a[0]).unwrap(), Coord::new(a[1]).unwrap(),
            Coord::new(a[2]).unwrap(), Coord::new(a[3]).unwrap(),
            Coord::new(a[4]).unwrap(), Coord::new(a[5]).unwrap()];
        let _ = csn.at_path(&CoordPath::new(coords));
    }
    let read_csn = t.elapsed();

    let t = Instant::now();
    for (i, a) in axes.iter().enumerate() {
        let path = pack_6to2(a);
        let _ = cs2.at_path(&path);
    }
    let read_cs2 = t.elapsed();

    // iter_tree (full scan)
    let t = Instant::now();
    let mut count = 0u64;
    for (_p, v) in csn.iter_tree() { count += *v; }
    let scan_csn = t.elapsed();
    assert_eq!(count, (n as u64 * (n as u64 - 1) / 2) as u64);

    println!("  CoordSpaceN6     | {:>8?} | {:>10?} | {:>8?}", write_csn, read_csn, scan_csn);
    println!("  CoordSpace2      | {:>8?} | {:>10?} | {:>8?}", write_cs2, read_cs2, "N/A");

    // Show relative performance
    let csn_ns = read_csn.as_nanos() as f64 / n as f64;
    let cs2_ns = read_cs2.as_nanos() as f64 / n as f64;
    println!();
    println!("  at_path per entry:");
    println!("    CoordSpaceN6:  {:.1} ns", csn_ns);
    println!("    CoordSpace2:   {:.1} ns  ({:.0}x faster)", cs2_ns, csn_ns / cs2_ns);
}
