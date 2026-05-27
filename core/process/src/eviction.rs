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
/// Phase 1 (flush): exports hot data to cold storage using the
/// provided `FlushCursor`. When `cursor.last_flushed_at` is non-empty,
/// only data ingested after that timestamp is exported (incremental
/// flush). Pass an empty cursor for a full re-export.
///
/// Returns the updated `FlushCursor` so the caller can persist it
/// (e.g. in StorageSnapshot) for the next eviction cycle.
///
/// Phase 2 (evict): removes stale nodes from hot storage.
///
/// Cold failure is non-fatal, retry next iteration.
pub fn try_evict_flush(
    backend: &mut (impl nexus_model::EvictCapable + nexus_model::FlushCapable),
    cursor: &nexus_model::FlushCursor,
    threshold: usize,
    cutoff_secs: u64,
) -> Result<(u64, nexus_model::FlushCursor), String> {
    let size = nexus_model::EvictCapable::approximate_size(&*backend);
    if size < threshold {
        return Ok((0, cursor.clone()));
    }

    // Phase 1: flush — cold failure is non-fatal, retry next iteration
    let result = match backend.flush_since(cursor) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("flush failed (non-fatal): {e}");
            return Ok((0, cursor.clone()));
        }
    };

    // Phase 2: evict
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let cutoff = now_secs.saturating_sub(cutoff_secs);
    let evicted = backend.evict_before(&cutoff.to_string())?;

    Ok((evicted, result.new_cursor))
}
