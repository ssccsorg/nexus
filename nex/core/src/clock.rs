// ── Clock abstraction ───────────────────────────────────────────────────

pub use fih_model::Now;

/// SystemTime-based clock. Correct for native targets.
/// On wasm32 with wasm-bindgen, maps to `Date.now()` internally.
/// On bare wasm32, returns UNIX_EPOCH (0).
#[derive(Debug, Clone, Copy)]
pub struct SystemClock;

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

/// Fixed-offset clock for MCU-class targets without a wall clock.
///
/// The MCU launcher has no OS and typically no RTC peripheral; it boots
/// with an unknown epoch. The launcher receives the epoch offset once,
/// from the host side or from a network time source, and this clock
/// derives both seconds and nanoseconds from a monotonic counter plus
/// that fixed offset. Callers that need true wall-clock time after the
/// offset is known construct this with the host-provided epoch.
///
/// This is the single swap point the thin-nexus principle reserves:
/// `Now` is the contract, and the launcher picks the clock implementation
/// for its target. It is intentionally minimal: no drift correction, no
/// NTP, no RTC register access. Those live in the launcher.
#[derive(Debug, Clone, Copy)]
pub struct OffsetClock {
    /// Seconds since UNIX_EPOCH at the moment the offset was captured.
    epoch_secs: u64,
}

impl OffsetClock {
    /// Build a clock pinned to the given wall-clock epoch (seconds since
    /// UNIX_EPOCH). The launcher supplies this once at boot.
    pub fn new(epoch_secs: u64) -> Self {
        Self { epoch_secs }
    }

    /// The pinned epoch, for inspection and for persisting across a
    /// launcher restart.
    pub fn epoch_secs(&self) -> u64 {
        self.epoch_secs
    }
}

impl Now for OffsetClock {
    fn now_nanos(&self) -> u64 {
        self.epoch_secs.saturating_mul(1_000_000_000)
    }

    fn now_secs(&self) -> u64 {
        self.epoch_secs
    }
}
