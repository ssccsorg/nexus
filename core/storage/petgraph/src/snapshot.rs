// nexus-storage-petgraph — Serializable snapshot for R2/blob persistence.

use crate::weight::{EdgeWeight, NodeWeight};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Serializable state of a graph partition for blob storage (R2, S3, etc.).
///
/// A worker saves its partition via `to_snapshot()` and restores it
/// via `from_snapshot()` on the next invocation. No external database
/// is required — just a blob store and serde.
///
/// Contains:
/// - `graph`: the full petgraph structure (nodes + edges + properties)
/// - `claims`: agent->intent ownership map (derived from ClaimsTracker)
/// - `project_id`: partition identifier
#[derive(Clone, Serialize, Deserialize)]
pub struct StorageSnapshot {
    pub graph: petgraph::Graph<NodeWeight, EdgeWeight>,
    pub claims: HashMap<String, String>,
    pub project_id: String,
}

/// A backend that can export and import its full state as a snapshot.
///
/// Workers use this to persist their partition to blob storage (R2, S3)
/// and restore it on the next invocation — no external database needed.
pub trait Snapshottable {
    fn to_snapshot(&self) -> StorageSnapshot;
    fn from_snapshot(snapshot: StorageSnapshot) -> Self;
}
