// nexus-storage-petgraph — Serializable snapshot for R2/blob persistence.
//
// Format: bincode (binary, serde-based).
// Version: top-level `version` field enables forward migration.

use crate::weight::{EdgeWeight, NodeWeight};
use nexus_model::{FlushCursor, TaskStates};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Serializable state of a graph partition for blob storage (R2, S3, etc.).
///
/// A worker saves its partition via `to_snapshot()` and restores it
/// via `from_snapshot()` on the next invocation. No external database
/// is required — just a blob store and bincode.
///
/// Contains:
/// - `graph`: the full petgraph structure (nodes + edges + properties)
/// - `claims`: agent->intent ownership map (derived from ClaimsTracker)
/// - `project_id`: partition identifier
/// - `task_states`: serialized detector state for cross-worker continuity
/// - `flush_cursor`: last flush position (empty = full flush)
/// - `version`: schema version for migration support
#[derive(Clone, Serialize, Deserialize)]
pub struct StorageSnapshot {
    pub graph: petgraph::Graph<NodeWeight, EdgeWeight>,
    pub claims: HashMap<String, String>,
    pub project_id: String,
    #[serde(default)]
    pub task_states: TaskStates,
    #[serde(default)]
    pub flush_cursor: FlushCursor,
    /// Schema version. Current: 1.
    /// Increment when fields change incompatibly.
    #[serde(default = "default_version")]
    pub version: u32,
}

fn default_version() -> u32 {
    1
}

impl StorageSnapshot {
    /// Serialize to bincode bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        bincode::serialize(self).map_err(|e| format!("bincode serialize: {e}"))
    }

    /// Deserialize from bincode bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let snap: Self =
            bincode::deserialize(bytes).map_err(|e| format!("bincode deserialize: {e}"))?;
        match snap.version {
            1 => Ok(snap),
            v => Err(format!("unsupported snapshot version: {v}")),
        }
    }
}

/// A backend that can export and import its full state as a snapshot.
///
/// Workers use this to persist their partition to blob storage (R2, S3)
/// and restore it on the next invocation — no external database needed.
pub trait Snapshottable {
    fn to_snapshot(&self) -> StorageSnapshot;
    fn from_snapshot(snapshot: StorageSnapshot) -> Self;
}
