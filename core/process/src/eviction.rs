// nexus-process — Eviction: flush + evict_before cycle for hot memory management.
//
// The eviction cycle bounds hot storage memory by:
//   1. Checking `approximate_size()` against a threshold
//   2. Evicting stale nodes from the hot store
//
// This implements the Stigmergy pheromone evaporation metaphor:
// old signals decay over time, making room for new ones.

use crate::ProcessError;
use nexus_model::EvictCapable;

/// Run a single eviction check on the given backend.
/// Returns the number of evicted nodes.
pub fn try_evict(
    backend: &impl EvictCapable,
    threshold: usize,
) -> Result<u64, ProcessError> {
    let size = EvictCapable::approximate_size(backend);
    if size < threshold {
        return Ok(0);
    }

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let cutoff = now_secs.saturating_sub(3600);

    let removed = backend
        .evict_before(&cutoff.to_string())
        .map_err(|e| ProcessError::Eviction(e))?;

    Ok(removed)
}
