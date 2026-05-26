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
//
// When the backend also implements `FlushCapable`, the flush step uses
// the persisted `FlushCursor` so that only data ingested since the last
// completed flush is exported (incremental, not full re-export).

/// Eviction cycle: flush then evict when memory exceeds threshold.
///
/// Phase 1 (flush): uses `FlushCapable::flush_since` to export
/// hot data to cold storage. The cursor is always full (empty string)
/// at this level because cursor persistence belongs to the scheduler
/// or gateway layer, not the eviction helper.
///
/// For incremental flush (export only data since last flush), the
/// caller should invoke `DefaultBlackboard::flush()` (or the scheduler's
/// equivalent) instead of this helper.
///
/// Phase 2 (evict): removes stale nodes from hot storage.
///
/// Cold failure is non-fatal, retry next iteration.
pub fn try_evict_flush(
    backend: &mut (impl nexus_model::EvictCapable + nexus_model::FlushCapable),
    threshold: usize,
    cutoff_secs: u64,
) -> Result<u64, String> {
    let size = nexus_model::EvictCapable::approximate_size(&*backend);
    if size < threshold {
        return Ok(0);
    }

    // Phase 1: flush — cold failure is non-fatal, retry next iteration
    if let Err(e) = backend.flush_since(&nexus_model::FlushCursor {
        last_flushed_at: String::new(),
        partition: backend.project_id().to_string(),
    }) {
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
