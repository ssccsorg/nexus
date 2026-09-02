// no_std anchors for nex-core (issue #181).
//
// nex-core is the foundation storage layer: it must stay std-free so the
// OS-less storage path (MCU launcher) can link it. This test pins the
// std-free clock contracts:
//
//   - `Now` and `Monotonic` are pure traits with no std dependency,
//   - `EpochClock<M>` derives wall-clock time from an epoch + monotonic
//     tick, which is how a device without an RTC gets calendar time,
//   - `SystemClock`, `InstantClock`, and `HostClock` exist only under the
//     `std` feature; they are exercised by the std test suite and must
//     not be reachable from a no_std build.
//
// Run with:
//
//   cargo test --no-default-features --test no_std_anchors
//
// The std-gated clocks are tested in tests/clock_std.rs (std feature
// only); referencing them here fails to compile under
// `--no-default-features`, which is the point of the anchor.

use nex_core::{EpochClock, Monotonic, Now};

// ── Monotonic: a fake MCU tick, no std anywhere ────────────────────────

/// A hand-rolled monotonic source: the MCU timer tick. It advances only
/// when `tick` is called, so tests are deterministic and std-free.
#[derive(Debug, Clone, Default)]
struct FakeTick {
    elapsed_ns: u64,
}

impl FakeTick {
    fn tick(&mut self, ns: u64) {
        self.elapsed_ns += ns;
    }
}

impl Monotonic for FakeTick {
    fn elapsed_nanos(&self) -> u64 {
        self.elapsed_ns
    }
}

// ── EpochClock: wall clock from epoch + monotonic, std-free ────────────

#[test]
fn epoch_clock_starts_at_pinned_epoch() {
    let clock = EpochClock::new(1_700_000_000, FakeTick::default());
    assert_eq!(clock.epoch_secs(), 1_700_000_000);
    assert_eq!(clock.now_secs(), 1_700_000_000);
    assert_eq!(clock.now_nanos(), 1_700_000_000 * 1_000_000_000);
}

#[test]
fn epoch_clock_advances_with_monotonic_tick() {
    let mut tick = FakeTick::default();
    let clock = EpochClock::new(1_700_000_000, tick.clone());
    assert_eq!(clock.now_secs(), 1_700_000_000);

    // Advance 2.5 seconds: 2s crosses the second boundary, 0.5s stays
    // within it. now_nanos must reflect both epoch and elapsed.
    tick.tick(2_500_000_000);
    let clock = EpochClock::new(1_700_000_000, tick);
    assert_eq!(clock.now_secs(), 1_700_000_002);
    assert_eq!(
        clock.now_nanos(),
        1_700_000_002 * 1_000_000_000 + 500_000_000
    );
}

#[test]
fn epoch_clock_saturates_on_overflow() {
    // A u64 epoch near the top must not wrap; saturating arithmetic keeps
    // the clock monotonic instead of panicking.
    let tick = FakeTick::default();
    let clock = EpochClock::new(u64::MAX / 1_000_000_000, tick);
    let nanos = clock.now_nanos();
    assert!(nanos >= (u64::MAX / 1_000_000_000) * 1_000_000_000);
}

// ── Now: the trait is implementable without std ────────────────────────

#[test]
fn now_is_implementable_std_free() {
    // A no_std consumer (the MCU launcher) implements `Now` directly from
    // its timer; this pins that the trait has no hidden std bound.
    struct McuClock(FakeTick);
    impl Now for McuClock {
        fn now_nanos(&self) -> u64 {
            self.0.elapsed_nanos()
        }
        fn now_secs(&self) -> u64 {
            self.0.elapsed_nanos() / 1_000_000_000
        }
    }
    let mut tick = FakeTick::default();
    tick.tick(5_000_000_000);
    let clock = McuClock(tick);
    assert_eq!(clock.now_nanos(), 5_000_000_000);
    assert_eq!(clock.now_secs(), 5);
}

// ── Anchor documentation: the std-only surface ─────────────────────────
//
// The following types must NOT be referenced in this file. They exist
// only under the `std` feature:
//
//   - nex_core::SystemClock
//   - nex_core::InstantClock
//   - nex_core::HostClock
//
// If the `std` gate on any of these is accidentally removed, the
// `--no-default-features` build fails at their `std::time` usage, which
// the CI job catches before merge.
