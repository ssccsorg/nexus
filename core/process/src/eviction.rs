// nexus-process — Eviction: flush + evict_before cycle for hot memory management.
//
// The eviction cycle bounds hot storage memory by:
//   1. Checking `approximate_size()` against a threshold
//   2. (optional) Saving a snapshot via `SnapshotSaver` before evicting
//   3. Calling `evict_before(timestamp)` to remove stale nodes
//
// DATA SAFETY:
//   When a cold backend (DuckDB, SQLite) is configured, dual-write ensures
//   all data is already persisted — eviction is safe regardless of snapshot.
//   Without a cold backend, eviction loses data unless a `SnapshotSaver`
//   is provided to persist before removal.
//
// Generic over `impl EvictCapable`. Works with any storage backend
// that supports memory management.

use crate::error::ProcessError;
use nexus_model::EvictCapable;

/// Save a snapshot of the hot graph before eviction.
/// Implementations should serialise the current state to R2/Parquet.
pub trait SnapshotSaver {
    fn save(&self) -> Result<(), ProcessError>;
}

/// Run a single eviction check on the given backend.
/// Optionally saves a snapshot before evicting (recommended when no cold store).
/// Returns the number of evicted nodes.
///
/// When memory exceeds `threshold`, computes a cutoff timestamp
/// (2x heartbeat TTL before now) and evicts nodes older than that.
pub fn try_evict(
    backend: &impl EvictCapable,
    threshold: usize,
    snapshot: Option<&dyn SnapshotSaver>,
) -> Result<u64, ProcessError> {
    let size = EvictCapable::approximate_size(backend);
    if size < threshold {
        return Ok(0);
    }

    // Persist before removal (if saver provided)
    if let Some(s) = snapshot {
        s.save()?;
    }

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let cutoff = now_secs.saturating_sub(120);

    EvictCapable::evict_before(backend, &cutoff.to_string()).map_err(ProcessError::Eviction)
}
