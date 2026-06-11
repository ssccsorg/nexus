// ── Clock abstraction ───────────────────────────────────────────────────

/// Clock abstraction for platform-independent timestamp generation.
///
/// Implementations: SystemClock (native, real time), WasmClock (WASM,
/// monotonic-zero), FakeClock (deterministic testing), HybridLogicalClock
/// (distributed causal ordering).
///
/// A single trait injection point. Replace the clock, replace time semantics
/// for the entire system — no code in PetgraphStorage or Blackboard changes.
pub trait Now {
    /// Nanosecond-precision timestamp as u64.
    fn now_nanos(&self) -> u64;
    /// Second-precision timestamp as u64. Used for heartbeat expiry and
    /// eviction cutoffs.
    fn now_secs(&self) -> u64;
}

/// SystemTime-based clock. Correct for native targets.
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
