// ── Clock abstraction ───────────────────────────────────────────────────

/// Clock abstraction for platform-independent timestamp generation.
///
/// Implementations: SystemClock (native), js_sys::Date (WASM).
/// Without this trait, storage backends would be hardcoded to SystemTime::now(),
/// which is incorrect for WASM targets and makes testing impossible.
pub trait Now: Send + Sync {
    /// Return current time as a nanosecond-precision string.
    fn now_nanos(&self) -> String;
}

/// SystemTime-based clock. Correct for native targets.
#[derive(Debug, Clone, Copy)]
pub struct SystemClock;

impl Now for SystemClock {
    fn now_nanos(&self) -> String {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_string()
    }
}
