/// Utility types for nexus-graph.
///
/// Re-exported from the old nexus-table crate for backward compatibility.

/// Lightweight project metadata matching Cairn's Project schema.
#[derive(Debug, Clone)]
pub struct ProjectMeta {
    /// Unique project identifier.
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// Project status (active, stopped, completed).
    pub status: String,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
    /// Worker that initiated the project.
    pub reason_worker: Option<String>,
    /// Trigger that started the project.
    pub reason_trigger: Option<String>,
    /// ISO 8601 timestamp when the project was started.
    pub reason_started_at: Option<String>,
    /// ISO 8601 timestamp of the last heartbeat.
    pub reason_last_heartbeat_at: Option<String>,
}
