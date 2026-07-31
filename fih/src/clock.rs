// ── Clock abstraction ───────────────────────────────────────────────────

/// Clock abstraction for platform-independent timestamp generation.
pub trait Now {
    fn now_nanos(&self) -> u64;
    fn now_secs(&self) -> u64;
}
