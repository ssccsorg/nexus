//! The two renderings of an identity, per key.
//!
//! The native form is one character an axis. The ASCII form is three base-36
//! digits an axis, and both are 60 bytes at depth 20, so a switch between them
//! changes what a key looks like and not how long it is. What it could change is
//! the cost of turning an identity into a key and reading one back, and that is
//! paid once per key on the read path, so it is measured rather than argued.
//!
//! Run with `cargo bench -p nexus-bench --bench id_render`.

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use nex_fih::ascii;
use nex_fih::{CoordId, FihId20 as _};

fn id(seed: u64) -> CoordId {
    CoordId::from_label(&format!("bench-id-{seed}"))
}

fn renderings(c: &mut Criterion) {
    let id = id(7);
    let native = id.to_string();
    let rendered = ascii::encode(&id);
    assert_eq!(
        native.len(),
        rendered.len(),
        "the two renderings are the same number of bytes"
    );

    let mut group = c.benchmark_group("id/render");
    group.bench_function("native_encode", |b| {
        b.iter(|| black_box(black_box(&id).to_string()))
    });
    group.bench_function("ascii_encode", |b| {
        b.iter(|| black_box(ascii::encode(black_box(&id))))
    });
    group.bench_function("native_decode", |b| {
        b.iter(|| black_box(CoordId::<20>::from_string(black_box(&native))))
    });
    group.bench_function("ascii_decode", |b| {
        b.iter(|| black_box(ascii::decode::<20>(black_box(&rendered))))
    });
    group.finish();
}

criterion_group!(benches, renderings);
criterion_main!(benches);
