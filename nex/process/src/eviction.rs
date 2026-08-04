// nexus-process — Eviction: evict_before cycle for hot memory management.
//
// The eviction cycle bounds hot storage memory by:
//   1. Checking `approximate_size()` against a threshold
//   2. Evicting stale nodes from the store
//
// Eviction is durable: `evict_before` deletes the record files from io,
// and the OODA tick reads state (which flushes pending writes) before
// eviction runs, so no durable data is lost by the cycle. This
// implements the Stigmergy pheromone evaporation metaphor: old signals
// decay over time, making room for new ones.

/// Eviction cycle: evict when memory exceeds threshold.
///
/// Removes stale records (older than `cutoff_secs`) once the store size
/// passes `threshold`. Returns the number of evicted records.
///
/// Failure is non-fatal, retry next iteration.
pub fn try_evict(
    backend: &mut impl nex_fih::EvictCapable,
    threshold: usize,
    cutoff_secs: u64,
) -> Result<u64, String> {
    let size = nex_fih::EvictCapable::approximate_size(&*backend);
    if size < threshold {
        return Ok(0);
    }
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let cutoff = now_secs.saturating_sub(cutoff_secs);
    backend.evict_before(&cutoff.to_string())
}
