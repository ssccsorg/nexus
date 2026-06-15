// ── Clock abstraction ───────────────────────────────────────────────────

/// Clock abstraction for platform-independent timestamp generation.
/// Implementations live in storage backends, not in model.
pub trait Now {
    fn now_nanos(&self) -> u64;
    fn now_secs(&self) -> u64;
}
