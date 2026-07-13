use criterion::{black_box, criterion_group, criterion_main, Criterion};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const N: usize = tagma_core::TagmaCoord::N_VALID;

/// Generate N pre-computed valid coordinates.
fn all_coords() -> Vec<tagma_core::TagmaCoord> {
    (0..N as u16)
        .map(|i| tagma_core::TagmaCoord::new(i).unwrap())
        .collect()
}

/// N coordinates in random order.
fn shuffled_coords() -> Vec<tagma_core::TagmaCoord> {
    use rand::seq::SliceRandom;
    let mut rng = rand::thread_rng();
    let mut coords = all_coords();
    coords.shuffle(&mut rng);
    coords
}

/// A mixed workload: interleaved insert / update / remove / get.
/// Each operation touches a random coordinate.
fn mixed_workload(count: usize) -> Vec<MixedOp> {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let mut ops = Vec::with_capacity(count);
    for _ in 0..count {
        let coord = tagma_core::TagmaCoord::new(rng.gen_range(0..N as u16)).unwrap();
        let kind = rng.gen_range(0..4);
        ops.push(MixedOp { coord, kind });
    }
    ops
}

struct MixedOp {
    coord: tagma_core::TagmaCoord,
    kind: u8, // 0=insert, 1=get, 2=remove, 3=update
}

// ===========================================================================
// Insert microbenchmarks
// ===========================================================================

fn bench_tagma_insert_all(c: &mut Criterion) {
    let coords = all_coords();
    c.bench_function("TagmaMap/insert/all_11172", |b| {
        b.iter(|| {
            let mut map = tagma_core::TagmaMap::new();
            for &coord in &coords {
                black_box(map.insert(coord, coord.index()));
            }
            black_box(map);
        })
    });
}

fn bench_std_insert_all(c: &mut Criterion) {
    let coords = all_coords();
    c.bench_function("HashMap/insert/all_11172", |b| {
        b.iter(|| {
            let mut map = std::collections::HashMap::new();
            for &coord in &coords {
                black_box(map.insert(coord, coord.index()));
            }
            black_box(map);
        })
    });
}

fn bench_tagma_insert_random(c: &mut Criterion) {
    let coords = shuffled_coords();
    c.bench_function("TagmaMap/insert/random_11172", |b| {
        b.iter(|| {
            let mut map = tagma_core::TagmaMap::new();
            for &coord in &coords {
                black_box(map.insert(coord, coord.index()));
            }
            black_box(map);
        })
    });
}

fn bench_std_insert_random(c: &mut Criterion) {
    let coords = shuffled_coords();
    c.bench_function("HashMap/insert/random_11172", |b| {
        b.iter(|| {
            let mut map = std::collections::HashMap::new();
            for &coord in &coords {
                black_box(map.insert(coord, coord.index()));
            }
            black_box(map);
        })
    });
}

// ===========================================================================
// Get microbenchmarks
// ===========================================================================

fn bench_tagma_get_all(c: &mut Criterion) {
    let coords = all_coords();
    let mut map = tagma_core::TagmaMap::new();
    for &coord in &coords {
        map.insert(coord, coord.index());
    }
    c.bench_function("TagmaMap/get/all_11172", |b| {
        b.iter(|| {
            for &coord in &coords {
                black_box(black_box(&map).get(coord));
            }
        })
    });
}

fn bench_std_get_all(c: &mut Criterion) {
    let coords = all_coords();
    let mut map: std::collections::HashMap<_, _> = std::collections::HashMap::new();
    for &coord in &coords {
        map.insert(coord, coord.index());
    }
    c.bench_function("HashMap/get/all_11172", |b| {
        b.iter(|| {
            for &coord in &coords {
                black_box(black_box(&map).get(&coord));
            }
        })
    });
}

// ===========================================================================
// Overwrite (insert on full map)
// ===========================================================================

fn bench_tagma_overwrite_all(c: &mut Criterion) {
    let coords = all_coords();
    let mut map = tagma_core::TagmaMap::new();
    for &coord in &coords {
        map.insert(coord, 0);
    }
    c.bench_function("TagmaMap/overwrite/all_11172", |b| {
        b.iter(|| {
            for &coord in &coords {
                black_box(map.insert(coord, coord.index()));
            }
        })
    });
}

fn bench_std_overwrite_all(c: &mut Criterion) {
    let coords = all_coords();
    let mut map: std::collections::HashMap<_, _> = std::collections::HashMap::new();
    for &coord in &coords {
        map.insert(coord, 0);
    }
    c.bench_function("HashMap/overwrite/all_11172", |b| {
        b.iter(|| {
            for &coord in &coords {
                black_box(map.insert(coord, coord.index()));
            }
        })
    });
}

// ===========================================================================
// Remove all
// ===========================================================================

fn bench_tagma_remove_all(c: &mut Criterion) {
    let coords = all_coords();
    let mut map = tagma_core::TagmaMap::new();
    for &coord in &coords {
        map.insert(coord, coord.index());
    }
    c.bench_function("TagmaMap/remove/all_11172", |b| {
        b.iter(|| {
            let mut m = map.clone();
            for &coord in &coords {
                black_box(m.remove(coord));
            }
            black_box(m);
        })
    });
}

fn bench_std_remove_all(c: &mut Criterion) {
    let coords = all_coords();
    let mut map: std::collections::HashMap<_, _> = std::collections::HashMap::new();
    for &coord in &coords {
        map.insert(coord, coord.index());
    }
    c.bench_function("HashMap/remove/all_11172", |b| {
        b.iter(|| {
            let mut m = map.clone();
            for &coord in &coords {
                black_box(m.remove(&coord));
            }
            black_box(m);
        })
    });
}

// ===========================================================================
// Iteration
// ===========================================================================

fn bench_tagma_iter(c: &mut Criterion) {
    let coords = all_coords();
    let mut map = tagma_core::TagmaMap::new();
    for &coord in &coords {
        map.insert(coord, coord.index());
    }
    c.bench_function("TagmaMap/iter/all_11172", |b| {
        b.iter(|| {
            for (k, v) in black_box(&map) {
                black_box((k, v));
            }
        })
    });
}

fn bench_std_iter(c: &mut Criterion) {
    let coords = all_coords();
    let mut map: std::collections::HashMap<_, _> = std::collections::HashMap::new();
    for &coord in &coords {
        map.insert(coord, coord.index());
    }
    c.bench_function("HashMap/iter/all_11172", |b| {
        b.iter(|| {
            for (k, v) in black_box(&map) {
                black_box((k, v));
            }
        })
    });
}

// ===========================================================================
// Entry API
// ===========================================================================

fn bench_tagma_entry(c: &mut Criterion) {
    let coords = all_coords();
    c.bench_function("TagmaMap/entry/all_11172", |b| {
        b.iter(|| {
            let mut map = tagma_core::TagmaMap::new();
            for &coord in &coords {
                map.entry(coord).or_insert_with(|| coord.index());
            }
            black_box(map);
        })
    });
}

fn bench_std_entry(c: &mut Criterion) {
    let coords = all_coords();
    c.bench_function("HashMap/entry/all_11172", |b| {
        b.iter(|| {
            let mut map: std::collections::HashMap<_, _> = std::collections::HashMap::new();
            for &coord in &coords {
                map.entry(coord).or_insert_with(|| coord.index());
            }
            black_box(map);
        })
    });
}

// ===========================================================================
// Retain
// ===========================================================================

fn bench_tagma_retain_half(c: &mut Criterion) {
    let coords = all_coords();
    let mut map = tagma_core::TagmaMap::new();
    for &coord in &coords {
        map.insert(coord, coord.index());
    }
    c.bench_function("TagmaMap/retain/half", |b| {
        b.iter(|| {
            let mut m = map.clone();
            m.retain(|_, v| *v % 2 == 0);
            black_box(m);
        })
    });
}

fn bench_std_retain_half(c: &mut Criterion) {
    let coords = all_coords();
    let mut map: std::collections::HashMap<_, _> = std::collections::HashMap::new();
    for &coord in &coords {
        map.insert(coord, coord.index());
    }
    c.bench_function("HashMap/retain/half", |b| {
        b.iter(|| {
            let mut m = map.clone();
            m.retain(|_, v| *v % 2 == 0);
            black_box(m);
        })
    });
}

// ===========================================================================
// Drain (then reuse)
// ===========================================================================

fn bench_tagma_drain_all(c: &mut Criterion) {
    let coords = all_coords();
    let mut map = tagma_core::TagmaMap::new();
    for &coord in &coords {
        map.insert(coord, coord.index());
    }
    c.bench_function("TagmaMap/drain/all_11172", |b| {
        b.iter(|| {
            let mut m = map.clone();
            for (k, v) in m.drain() {
                black_box((k, v));
            }
            black_box(m);
        })
    });
}

fn bench_std_drain_all(c: &mut Criterion) {
    let coords = all_coords();
    let mut map: std::collections::HashMap<_, _> = std::collections::HashMap::new();
    for &coord in &coords {
        map.insert(coord, coord.index());
    }
    c.bench_function("HashMap/drain/all_11172", |b| {
        b.iter(|| {
            let mut m = map.clone();
            for (k, v) in m.drain() {
                black_box((k, v));
            }
            black_box(m);
        })
    });
}

// ===========================================================================
// Single-operation microbenchmarks (isolated, no loop overhead)
// ===========================================================================

fn bench_tagma_get_single(c: &mut Criterion) {
    let coord = tagma_core::TagmaCoord::new(5000).unwrap();
    let mut map = tagma_core::TagmaMap::new();
    map.insert(coord, 42u64);
    let map = map; // freeze

    c.bench_function("TagmaMap/get/single", |b| {
        b.iter(|| {
            black_box(black_box(&map).get(black_box(coord)));
        })
    });
}

fn bench_std_get_single(c: &mut Criterion) {
    use std::collections::HashMap;
    let coord = tagma_core::TagmaCoord::new(5000).unwrap();
    let mut map = HashMap::new();
    map.insert(coord, 42u64);
    let map = map;

    c.bench_function("HashMap/get/single", |b| {
        b.iter(|| {
            black_box(black_box(&map).get(black_box(&coord)));
        })
    });
}

fn bench_tagma_insert_single(c: &mut Criterion) {
    let coord = tagma_core::TagmaCoord::new(5000).unwrap();
    c.bench_function("TagmaMap/insert/single", |b| {
        b.iter(|| {
            let mut map = tagma_core::TagmaMap::new();
            black_box(map.insert(black_box(coord), 42u64));
        })
    });
}

fn bench_std_insert_single(c: &mut Criterion) {
    use std::collections::HashMap;
    let coord = tagma_core::TagmaCoord::new(5000).unwrap();
    c.bench_function("HashMap/insert/single", |b| {
        b.iter(|| {
            let mut map = HashMap::new();
            black_box(map.insert(black_box(coord), 42u64));
        })
    });
}

// ===========================================================================
// Stress test: 500,000 mixed operations on each map type
// ===========================================================================

fn bench_tagma_mixed_500k(c: &mut Criterion) {
    let ops = mixed_workload(500_000);
    let map = tagma_core::TagmaMap::new();

    c.bench_function("TagmaMap/stress/mixed_500k", |b| {
        b.iter(|| {
            let mut m = map.clone();
            for op in &ops {
                match op.kind {
                    0 => {
                        black_box(m.insert(op.coord, 1));
                    }
                    1 => {
                        black_box(m.get(op.coord));
                    }
                    2 => {
                        black_box(m.remove(op.coord));
                    }
                    _ => {
                        black_box(m.insert(op.coord, 2));
                    }
                }
            }
            black_box(m);
        })
    });
}

fn bench_std_mixed_500k(c: &mut Criterion) {
    use std::collections::HashMap;
    let ops = mixed_workload(500_000);
    let map: HashMap<tagma_core::TagmaCoord, u32> = HashMap::new();

    c.bench_function("HashMap/stress/mixed_500k", |b| {
        b.iter(|| {
            let mut m = map.clone();
            for op in &ops {
                match op.kind {
                    0 => {
                        black_box(m.insert(op.coord, 1));
                    }
                    1 => {
                        black_box(m.get(&op.coord));
                    }
                    2 => {
                        black_box(m.remove(&op.coord));
                    }
                    _ => {
                        black_box(m.insert(op.coord, 2));
                    }
                }
            }
            black_box(m);
        })
    });
}

// ===========================================================================
// Noop overhead baseline: just iterate the coordinate vec
// ===========================================================================

fn bench_baseline_iterate(c: &mut Criterion) {
    let coords = all_coords();
    c.bench_function("baseline/iterate_N", |b| {
        b.iter(|| {
            for &coord in &coords {
                black_box(coord);
            }
        })
    });
}

// ===========================================================================
// Group runner
// ===========================================================================

criterion_group!(
    name = inserts;
    config = Criterion::default().sample_size(100).measurement_time(std::time::Duration::from_secs(20));
    targets = bench_tagma_insert_all, bench_std_insert_all,
              bench_tagma_insert_random, bench_std_insert_random
);
criterion_group!(
    name = lookup;
    config = Criterion::default().sample_size(100).measurement_time(std::time::Duration::from_secs(20));
    targets = bench_tagma_get_all, bench_std_get_all,
              bench_tagma_overwrite_all, bench_std_overwrite_all
);
criterion_group!(
    name = mutate;
    config = Criterion::default().sample_size(100).measurement_time(std::time::Duration::from_secs(20));
    targets = bench_tagma_remove_all, bench_std_remove_all,
              bench_tagma_entry, bench_std_entry,
              bench_tagma_retain_half, bench_std_retain_half
);
criterion_group!(
    name = iterate;
    config = Criterion::default().sample_size(100).measurement_time(std::time::Duration::from_secs(20));
    targets = bench_tagma_iter, bench_std_iter,
              bench_tagma_drain_all, bench_std_drain_all,
              bench_baseline_iterate
);
criterion_group!(
    name = micro;
    config = Criterion::default().sample_size(1000).measurement_time(std::time::Duration::from_secs(6));
    targets = bench_tagma_get_single, bench_std_get_single,
              bench_tagma_insert_single, bench_std_insert_single
);
criterion_group!(
    name = stress;
    config = Criterion::default().sample_size(30).measurement_time(std::time::Duration::from_secs(30));
    targets = bench_tagma_mixed_500k, bench_std_mixed_500k
);

criterion_main!(inserts, lookup, mutate, iterate, micro, stress);
