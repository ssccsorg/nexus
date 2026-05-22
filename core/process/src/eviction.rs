// nexus-process — Eviction: flush + evict_before cycle for hot memory management.
//
// The eviction cycle bounds hot storage memory by:
//   1. Checking `approximate_size()` against a threshold
//   2. Calling `evict_before(timestamp)` to remove stale nodes
//
// This implements the Stigmergy pheromone evaporation metaphor:
// old signals decay over time, making room for new ones.
//
// Generic over `impl EvictCapable`. Works with any storage backend
// that supports memory management.

use nexus_model::EvictCapable;

/// Run a single eviction check on the given backend.
/// Returns the number of evicted nodes.
///
/// When memory exceeds `threshold`, computes a cutoff timestamp
/// (currently now minus retention) and evicts nodes older than that.
pub fn try_evict(backend: &impl EvictCapable, threshold: usize) -> Result<u64, String> {
    let size = EvictCapable::approximate_size(backend);
    if size < threshold {
        return Ok(0); // under threshold, no eviction needed
    }

    // Cutoff: 2x heartbeat TTL (120s default) before now
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let cutoff = now_secs.saturating_sub(120);

    EvictCapable::evict_before(backend, &cutoff.to_string())
}
