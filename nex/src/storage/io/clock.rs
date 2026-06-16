// ── SystemClock: std::time-based clock ─────────────────────────────────
//
// Works on all targets. On wasm32 with wasm-bindgen, `SystemTime::now()`
// maps to `Date.now()` internally. On bare wasm32, returns UNIX_EPOCH (0).

use nexus_model::Now;

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
