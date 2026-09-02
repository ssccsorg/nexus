// EpochClock unit tests (issue #181, MCU wall-clock source).
//
// EpochClock pins a wall-clock epoch at boot and advances from a
// monotonic tick. These tests pin the time flow, the second/nanosecond
// conversion, and the saturating arithmetic at the u64 boundary that an
// MCU without an RTC relies on.

use nex_core::{EpochClock, Monotonic, Now};

#[derive(Debug, Clone, Default)]
struct FakeTick(u64);

impl Monotonic for FakeTick {
    fn elapsed_nanos(&self) -> u64 {
        self.0
    }
}

#[test]
fn epoch_baseline_at_zero_tick() {
    let clock = EpochClock::new(1_700_000_000, FakeTick(0));
    assert_eq!(clock.now_nanos(), 1_700_000_000_000_000_000);
    assert_eq!(clock.now_secs(), 1_700_000_000);
    assert_eq!(clock.epoch_secs(), 1_700_000_000);
}

#[test]
fn time_advances_with_tick() {
    let clock = EpochClock::new(1_700_000_000, FakeTick(5_000_000_000));
    assert_eq!(clock.now_nanos(), 1_700_000_000_000_000_000 + 5_000_000_000);
    // 5 seconds of elapsed nanos advance now_secs by 5.
    assert_eq!(clock.now_secs(), 1_700_000_000 + 5);
}

#[test]
fn sub_second_elapsed_does_not_advance_secs() {
    let clock = EpochClock::new(1_700_000_000, FakeTick(999_999_999));
    assert_eq!(clock.now_secs(), 1_700_000_000);
    assert_eq!(clock.now_nanos(), 1_700_000_000_999_999_999);
}

#[test]
fn now_nanos_saturates_at_u64_boundary() {
    // epoch seconds near u64::MAX: saturating_mul must not panic or wrap.
    let clock = EpochClock::new(u64::MAX, FakeTick(1_000));
    assert_eq!(clock.now_nanos(), u64::MAX);
    assert_eq!(clock.now_secs(), u64::MAX);
}

#[test]
fn now_nanos_saturates_on_elapsed_overflow() {
    let clock = EpochClock::new(1_700_000_000, FakeTick(u64::MAX));
    // epoch * 1e9 + elapsed saturates instead of wrapping.
    assert_eq!(clock.now_nanos(), u64::MAX);
}

#[test]
fn now_secs_saturates_on_elapsed_overflow() {
    // A large elapsed (in whole seconds) plus a near-max epoch must
    // saturate in both now_nanos and now_secs consistently.
    let epoch = u64::MAX - 5;
    let clock = EpochClock::new(epoch, FakeTick(50_000_000_000));
    assert_eq!(clock.now_secs(), u64::MAX);
    assert_eq!(clock.now_nanos(), u64::MAX);
}

#[test]
fn monotonic_in_tick() {
    // Advancing the tick must never move the clock backwards.
    let mut tick = 0u64;
    let clock = EpochClock::new(1_700_000_000, FakeTick(tick));
    let mut prev = clock.now_nanos();
    for step in [1, 10, 100, 1_000, 1_000_000, 1_000_000_000] {
        tick += step;
        let clock = EpochClock::new(1_700_000_000, FakeTick(tick));
        let now = clock.now_nanos();
        assert!(now >= prev, "clock moved backwards");
        prev = now;
    }
}
