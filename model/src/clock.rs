// ── Clock abstraction ───────────────────────────────────────────────────

/// Clock abstraction for platform-independent timestamp generation.
pub trait Now {
    fn now_nanos(&self) -> u64;
    fn now_secs(&self) -> u64;
}

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
