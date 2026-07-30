// ═══════════════════════════════════════════════════════════════════════════
// nexus-storage-sim Benchmarks
// Run: cargo test --release -p nexus-storage-sim --bench bench -- --nocapture
// ═══════════════════════════════════════════════════════════════════════════

use futures_executor::block_on;
use nex_fih::{
    AsyncFactCapable, AsyncFilterCapable, AsyncStorageRead, AxisHints, CoordId, Fact, FihStorage,
    StateFilter,
};
use nexus_storage_sim::SimIo;
use std::time::Instant;

// ═══════════════════════════════════════════════════════════════════════════
// Section 1: AND Query — iter_prefix vs full scan (raw CoordSpaceN<6, u32>)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn raw_iter_prefix_vs_full_scan() {
    use tagma_core::{Coord, CoordPath, CoordSpaceN};

    let mut space: CoordSpaceN<6, u32> = CoordSpaceN::new();

    for i in 0..50_000u32 {
        let time_hi = (i / 10000) as u16;
        let time_lo = ((i / 100) % 100) as u16;
        let origin = (i % 200) as u16;
        let creator = (i % 100) as u16;
        let serial = (i % 1000) as u16;
        space.place_path(
            &CoordPath::new([
                Coord::new(time_hi).unwrap(),
                Coord::new(time_lo).unwrap(),
                Coord::new(0).unwrap(),
                Coord::new(origin).unwrap(),
                Coord::new(creator).unwrap(),
                Coord::new(serial).unwrap(),
            ]),
            i,
        );
    }

    let t = Instant::now();
    for (_p, _v) in space.iter_tree() {}
    let full = t.elapsed();

    let t = Instant::now();
    for th in 0..5u16 {
        for tl in 0..100u16 {
            if let Some(iter) = space.iter_prefix(&[
                Coord::new(th).unwrap(),
                Coord::new(tl).unwrap(),
                Coord::new(0).unwrap(),
                Coord::new(7).unwrap(),
                Coord::new(7).unwrap(),
            ]) {
                for (_p, _v) in iter {}
            }
        }
    }
    let prefix = t.elapsed();

    println!(
        "[RAW] full={:?} prefix={:?} (iter_prefix, 5×100 prefixes)",
        full, prefix,
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 2: FihStorage — fast-path HashMaps vs full scan
// ═══════════════════════════════════════════════════════════════════════════

fn populate_10k(store: &FihStorage<SimIo>) -> u64 {
    let clock = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    for i in 0..10_000u32 {
        let cid = CoordId::from_axes(
            ((clock + i as u64) / 86_400_000_000_000 % 11172) as u16,
            ((clock + i as u64) % 86_400_000_000_000 % 11172) as u16,
            0,
            (i % 50) as u16,
            (i % 20) as u16,
            (i / 1000) as u16,
        )
        .unwrap();
        block_on(store.submit_fact(&Fact::with_id(
            cid,
            format!("origin-{}", i % 50),
            format!("c-{}", i).into(),
            format!("creator-{}", i % 20),
        )))
        .unwrap();
    }
    block_on(store.flush_pending()).unwrap();
    clock
}

#[test]
fn fih_benchmark_10k() {
    let io = SimIo::new();
    let store = FihStorage::new(io, "b10k");
    let t = Instant::now();
    populate_10k(&store);
    let tw = t.elapsed();

    let t = Instant::now();
    let s = block_on(store.read_state());
    let tr = t.elapsed();
    println!(
        "[10K] write={:?} read_state={:?} ({} facts)",
        tw,
        tr,
        s.facts.len()
    );

    let t = Instant::now();
    for _ in 0..10 {
        block_on(store.read_state_filtered(&StateFilter {
            creator: Some("creator-5".into()),
            ..Default::default()
        }));
    }
    println!(
        "[10K] filter creator 10x={:?} avg={:?}",
        t.elapsed(),
        t.elapsed() / 10
    );

    let t = Instant::now();
    for _ in 0..10 {
        block_on(store.read_state_filtered(&StateFilter {
            origin: Some("origin-7".into()),
            creator: Some("creator-7".into()),
            ..Default::default()
        }));
    }
    println!(
        "[10K] AND query 10x={:?} avg={:?}",
        t.elapsed(),
        t.elapsed() / 10
    );
}

fn populate_50k(store: &FihStorage<SimIo>) {
    for i in 0..50_000u32 {
        block_on(store.submit_fact(&Fact::with_id(
            CoordId::new(i as u64),
            format!("origin-{}", i % 500),
            format!("c-{}", i).into(),
            format!("creator-{}", i % 200),
        )))
        .unwrap();
    }
    block_on(store.flush_pending()).unwrap();
}

#[test]
fn fih_benchmark_50k() {
    let io = SimIo::new();
    let store = FihStorage::new(io, "b50k");
    let t = Instant::now();
    populate_50k(&store);
    println!("[50K] write={:?}", t.elapsed());

    let t = Instant::now();
    for _ in 0..10 {
        block_on(store.read_state_filtered(&StateFilter {
            creator: Some("creator-7".into()),
            ..Default::default()
        }));
    }
    println!(
        "[50K] filter creator 10x={:?} avg={:?}",
        t.elapsed(),
        t.elapsed() / 10
    );

    let t = Instant::now();
    for _ in 0..10 {
        block_on(store.read_state_filtered(&StateFilter {
            origin: Some("origin-7".into()),
            creator: Some("creator-7".into()),
            ..Default::default()
        }));
    }
    println!(
        "[50K] AND query 10x={:?} avg={:?}",
        t.elapsed(),
        t.elapsed() / 10
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 3: axis_hints — CoordId-based iter_prefix fast path
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn axis_hints_bench() {
    let io = SimIo::new();
    let store = FihStorage::new(io, "hints");
    let clock = populate_10k(&store);

    let th = ((clock) / 86_400_000_000_000 % 11172) as u16;
    let tl = ((clock) % 86_400_000_000_000 % 11172) as u16;

    // Without hints: full scan
    let t = Instant::now();
    let s = block_on(store.read_state_filtered(&StateFilter {
        creator: Some("creator-5".into()),
        ..Default::default()
    }));
    let no_hints = t.elapsed();

    // With axis_hints: iter_prefix
    let t = Instant::now();
    let s2 = block_on(store.read_state_filtered(&StateFilter {
        origin: Some("origin-7".into()),
        creator: Some("creator-7".into()),
        axis_hints: Some(AxisHints {
            time_hi: Some(th),
            time_lo: Some(tl),
            entity: Some(0),
            origin: Some(7),
            creator: Some(7),
            serial: None,
        }),
        ..Default::default()
    }));
    let with_hints = t.elapsed();

    println!(
        "[HINTS] no_hints={:?} ({}) with_hints={:?} ({}) speedup={:.0}x",
        no_hints,
        s.facts.len(),
        with_hints,
        s2.facts.len(),
        no_hints.as_nanos() as f64 / with_hints.as_nanos().max(1) as f64
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Section 4: CoordSpace type comparison — N6 (tree) vs CS2 (dense)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn coord_space_type_cmp() {
    use tagma_core::{Coord, CoordPath, CoordSpace2, CoordSpaceN};

    let n = 10_000u32;
    let axes: Vec<[u16; 6]> = (0..n)
        .map(|i| {
            let clock = 1700000000000000u64;
            [
                ((clock + i as u64) / 86_400_000_000_000 % 11172) as u16,
                ((clock + i as u64) % 86_400_000_000_000 % 11172) as u16,
                0u16,
                (i % 500) as u16,
                (i % 200) as u16,
                (i / 1000) as u16,
            ]
        })
        .collect();

    // CoordSpaceN6 (tree)
    let mut csn: CoordSpaceN<6, u64> = CoordSpaceN::new();
    for (i, a) in axes.iter().enumerate() {
        csn.place_path(
            &CoordPath::new([
                Coord::new(a[0]).unwrap(),
                Coord::new(a[1]).unwrap(),
                Coord::new(a[2]).unwrap(),
                Coord::new(a[3]).unwrap(),
                Coord::new(a[4]).unwrap(),
                Coord::new(a[5]).unwrap(),
            ]),
            i as u64,
        );
    }
    let t = Instant::now();
    for a in axes.iter() {
        let _ = csn.at_path(&CoordPath::new([
            Coord::new(a[0]).unwrap(),
            Coord::new(a[1]).unwrap(),
            Coord::new(a[2]).unwrap(),
            Coord::new(a[3]).unwrap(),
            Coord::new(a[4]).unwrap(),
            Coord::new(a[5]).unwrap(),
        ]));
    }
    let csn_read = t.elapsed();

    // CoordSpace2 (dense heap, 6→2 axes packed)
    let pack = |a: &[u16; 6]| {
        CoordPath::new([
            Coord::new(((a[0] as u64 * 11172 + a[1] as u64) % 11172) as u16).unwrap(),
            Coord::new(
                ((((a[2] as u64 * 11172 + a[3] as u64) * 11172 + a[4] as u64) * 11172
                    + a[5] as u64)
                    % 11172) as u16,
            )
            .unwrap(),
        ])
    };
    let mut cs2: CoordSpace2<u64> = CoordSpace2::new();
    for (i, a) in axes.iter().enumerate() {
        cs2.place_path(&pack(a), i as u64);
    }
    let t = Instant::now();
    for a in axes.iter() {
        let _ = cs2.at_path(&pack(a));
    }
    let cs2_read = t.elapsed();

    println!(
        "[CSTYPE] CS2 per-op={:.0}ns CS-N6 per-op={:.0}ns ratio={:.0}x",
        cs2_read.as_nanos() as f64 / n as f64,
        csn_read.as_nanos() as f64 / n as f64,
        csn_read.as_nanos() as f64 / cs2_read.as_nanos().max(1) as f64
    );
}
