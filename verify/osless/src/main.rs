// OS-less storage path verification on the host (issue #181).
//
// This crate is support software: it exercises the no_std storage core
// (nex-core, nex-fih, chton) built with the std feature off, driving it
// from a caller-provided critical-section implementation exactly as an
// MCU firmware or launcher would. The host harness supplies a no-op
// critical section and a no-op waker, so nothing on the path needs std:
//   - Cell2 borrow semantics, including cross-thread serialization
//   - EpochClock time flow from a monotonic tick
//   - a FihStorage submit -> flush -> read round trip
//
// The compile-time no_std property is pinned by the no_std anchor tests in
// the nex sub-workspace; this crate verifies the runtime behavior of that
// same no_std build.

use std::sync::Arc;
use std::thread;

use chton::cell::Cell2;
use critical_section::RawRestoreState;
use nex_core::{EpochClock, Monotonic, Now};
use nex_fih::{AsyncFactCapable, AsyncStorageRead, BoardState, Content, Fact, FihStorage};
use nexus_verify_support::{FlatIo, block_on};

// ── Caller-provided critical section: no-op for the single-threaded
//    checks; the cross-thread check relies on the host critical-section
//    implementation being process-wide. This mirrors an MCU firmware that
//    supplies the symbols from a HAL. ───────────────────────────────────

struct NoopCs;
critical_section::set_impl!(NoopCs);

unsafe impl critical_section::Impl for NoopCs {
    unsafe fn acquire() -> RawRestoreState {
        false
    }
    unsafe fn release(_restore_state: RawRestoreState) {}
}

fn main() {
    // 1. Cell2: value round trip and independent cells.
    let cell = Cell2::new(0u64);
    *cell.borrow_mut() = 41;
    assert_eq!(*cell.borrow(), 41);
    let a = Cell2::new(1u64);
    let b = Cell2::new(2u64);
    {
        let ga = a.borrow();
        let gb = b.borrow();
        assert_eq!(*ga + *gb, 3);
    }
    *a.borrow_mut() = 10;
    assert_eq!(*b.borrow(), 2);

    // 2. Cell2 cross-thread serialization on the OS-less build.
    let shared = Arc::new(Cell2::new(0u64));
    let t1 = {
        let c = Arc::clone(&shared);
        thread::spawn(move || {
            for _ in 0..1000 {
                *c.borrow_mut() += 1;
            }
        })
    };
    let t2 = {
        let c = Arc::clone(&shared);
        thread::spawn(move || {
            for _ in 0..1000 {
                *c.borrow_mut() += 1;
            }
        })
    };
    t1.join().unwrap();
    t2.join().unwrap();
    assert_eq!(*shared.borrow(), 2000);

    // 3. EpochClock time flow from a monotonic tick.
    #[derive(Clone)]
    struct Tick(Arc<core::sync::atomic::AtomicU64>);
    impl Monotonic for Tick {
        fn elapsed_nanos(&self) -> u64 {
            self.0.load(core::sync::atomic::Ordering::Relaxed)
        }
    }
    let tick = Tick(Arc::new(core::sync::atomic::AtomicU64::new(0)));
    let epoch_secs: u64 = 1_700_000_000;
    let clock = EpochClock::new(epoch_secs, tick.clone());
    let epoch_ns = epoch_secs.saturating_mul(1_000_000_000);
    assert_eq!(clock.now_nanos(), epoch_ns);
    tick.0
        .store(5_000_000_000, core::sync::atomic::Ordering::Relaxed);
    assert_eq!(clock.now_nanos(), epoch_ns + 5_000_000_000);
    assert_eq!(clock.now_secs(), epoch_secs + 5);

    // 4. FihStorage round trip on the no_std build.
    let io = FlatIo::new();
    let storage = FihStorage::with_clock(io, "osless", Box::new(clock));

    let fact = Fact::new(
        "osless".into(),
        Content {
            mime_type: "text/plain".into(),
            data: b"hello osless".to_vec(),
        },
        "harness".into(),
    );
    let id = block_on(storage.submit_fact(&fact)).expect("submit_fact");
    block_on(storage.flush_pending()).expect("flush_pending");

    let state: BoardState = block_on(storage.read_state());
    let found = state
        .facts
        .iter()
        .any(|f| f.id == id && f.content.data == b"hello osless");
    assert!(found, "submitted fact not found in read-back state");

    println!("nexus-osless-verify: all OS-less surface checks passed");
}
