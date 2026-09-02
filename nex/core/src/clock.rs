// ── Clock abstraction ───────────────────────────────────────────────────
//
// The `Now` and `Monotonic` traits are the std-free clock contracts. Any
// target can implement them: native uses `std::time`, the launcher on an
// MCU uses its timer tick (embassy), and the host side can pin an epoch.
//
// `SystemClock` (std) is gated behind the `std` feature so the core crate
// itself stays no_std. `EpochClock<M>` combines a fixed wall-clock epoch
// with any `Monotonic` source, which is the standard way to get calendar
// time on a device without an RTC.

pub use fih_model::Now;

/// Monotonic time source: elapsed nanoseconds since an arbitrary baseline.
///
/// Always available on MCU targets (timer tick), even without an RTC.
/// This is the tick that `EpochClock` advances against.
pub trait Monotonic {
    /// Nanoseconds elapsed since this clock's baseline.
    fn elapsed_nanos(&self) -> u64;
}

/// Wall clock from a fixed epoch plus a monotonic source.
///
/// The launcher supplies the wall-clock epoch once at boot (from the host
/// or a network time source); time then advances from the monotonic tick,
/// so calendar time works on a device without an RTC. Drift correction,
/// NTP, and RTC register access live in the launcher, not here.
#[derive(Debug, Clone)]
pub struct EpochClock<M: Monotonic> {
    /// Seconds since UNIX_EPOCH at boot, supplied by the launcher.
    epoch_secs: u64,
    /// Monotonic source that advances time from the boot epoch.
    mono: M,
}

impl<M: Monotonic> EpochClock<M> {
    /// Build a clock pinned to the given wall-clock epoch (seconds since
    /// UNIX_EPOCH) advancing from `mono`.
    pub fn new(epoch_secs: u64, mono: M) -> Self {
        Self { epoch_secs, mono }
    }

    /// The pinned epoch, for inspection and persisting across restarts.
    pub fn epoch_secs(&self) -> u64 {
        self.epoch_secs
    }
}

impl<M: Monotonic> Now for EpochClock<M> {
    fn now_nanos(&self) -> u64 {
        self.epoch_secs
            .saturating_mul(1_000_000_000)
            .saturating_add(self.mono.elapsed_nanos())
    }

    fn now_secs(&self) -> u64 {
        // Saturating, consistent with now_nanos: a wall clock on an MCU
        // must be total even at the u64 boundary, not wrap.
        self.epoch_secs
            .saturating_add(self.mono.elapsed_nanos() / 1_000_000_000)
    }
}

/// SystemTime-based clock. Correct for native targets.
///
/// On wasm32 with wasm-bindgen, maps to `Date.now()` internally.
/// On bare wasm32, returns UNIX_EPOCH (0).
#[cfg(feature = "std")]
#[derive(Debug, Clone, Copy)]
pub struct SystemClock;

#[cfg(feature = "std")]
impl Now for SystemClock {
    fn now_nanos(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64
    }

    fn now_secs(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

/// Monotonic baseline backed by `std::time::Instant` (native / WASI).
#[cfg(feature = "std")]
#[derive(Debug, Clone)]
pub struct InstantClock {
    start: std::time::Instant,
}

#[cfg(feature = "std")]
impl InstantClock {
    pub fn new() -> Self {
        Self {
            start: std::time::Instant::now(),
        }
    }
}

#[cfg(feature = "std")]
impl Default for InstantClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "std")]
impl Monotonic for InstantClock {
    fn elapsed_nanos(&self) -> u64 {
        self.start.elapsed().as_nanos() as u64
    }
}

/// Convenience alias: the host-side wall clock (std epoch + std monotonic).
#[cfg(feature = "std")]
pub type HostClock = EpochClock<InstantClock>;
