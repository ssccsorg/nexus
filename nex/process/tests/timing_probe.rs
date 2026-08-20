// Timing probe: conflict-check latency in the record layer (#176).
//
// Measurement tooling, not part of the regular gate. Run with:
//
//   cargo test -p nex --test timing_probe -- --ignored --nocapture
//
// Issue #176 documented a conflict check of 135-390 ms per submit
// (10k, tree scan) before the fix and ~2 us after (id-keyed record map
// lookup). This probe re-measures the same paths after the L2
// restructure and the entity store merge: a conflicting submit (occupied
// id, different content), an idempotent retry, and a fresh submit.

use std::time::Instant;

use futures_executor::block_on;
use nex_fih::{AsyncFactCapable, BlackboardError, Content, CoordId, Fact, FihStorage};
use nexus_storage_sim::SimIo;

fn fact(id: &str, data: &[u8]) -> Fact {
    Fact::with_id(
        CoordId::from_label(id),
        "timing".into(),
        Content {
            mime_type: "text/plain".into(),
            data: data.to_vec(),
        },
        "t".into(),
    )
}

fn timed<F: FnMut()>(mut f: F, n: usize) -> std::time::Duration {
    let start = Instant::now();
    for _ in 0..n {
        f();
    }
    start.elapsed() / n as u32
}

#[test]
#[ignore]
fn report_conflict_check_latency() {
    const N: usize = 1_000;
    const M: usize = 200;

    let store = FihStorage::new(SimIo::new(), "timing-probe");
    for i in 0..N {
        block_on(store.submit_fact(&fact(&format!("f-{i}"), b"payload"))).unwrap();
    }

    // Conflict: the same id with different content must be rejected.
    let conflict = fact("f-0", b"different-content");
    let per_conflict = timed(
        || {
            let err = block_on(store.submit_fact(&conflict));
            assert!(matches!(err, Err(BlackboardError::Conflict(_))));
        },
        M,
    );

    // Idempotent: the same id with the identical content is a retry.
    let idem = fact("f-1", b"payload");
    let per_idem = timed(
        || {
            block_on(store.submit_fact(&idem)).unwrap();
        },
        M,
    );

    // Fresh submit against an unoccupied id (occupancy miss path).
    let fresh = fact("fresh", b"payload");
    let per_fresh = timed(
        || {
            block_on(store.submit_fact(&fresh)).unwrap();
        },
        M,
    );

    println!(
        "conflict check: {per_conflict:?} per call ({} calls); \
         idempotent: {per_idem:?}; fresh: {per_fresh:?}",
        M
    );
}
