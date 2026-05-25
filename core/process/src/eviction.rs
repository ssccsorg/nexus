// nexus-process — Eviction: flush + evict_before cycle for hot memory management.
//
// The eviction cycle bounds hot storage memory by:
//   1. Calling `flush()` first to confirm cold storage is synced
//   2. Checking `approximate_size()` against a threshold
//   3. Evicting stale nodes from the hot store
//
// Always flush first, then evict. Cold failure is non-fatal (retry next tick).
// This implements the Stigmergy pheromone evaporation metaphor:
// old signals decay over time, making room for new ones.

pub fn try_evict_flush(
    backend: &(impl nexus_model::EvictCapable + nexus_model::FlushCapable),
    threshold: usize,
    cutoff_secs: u64,
) -> Result<u64, String> {
    let size = nexus_model::EvictCapable::approximate_size(backend);
    if size < threshold {
        return Ok(0);
    }

    // Phase 1: flush — cold failure is non-fatal, retry next iteration
    let cursor = nexus_model::FlushCursor {
        last_flushed_at: String::new(),
        partition: backend.project_id().to_string(),
    };
    if let Err(e) = backend.flush_since(&cursor) {
        log::warn!("flush failed (non-fatal): {e}");
    }

    // Phase 2: evict
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let cutoff = now_secs.saturating_sub(cutoff_secs);
    backend.evict_before(&cutoff.to_string())
}
