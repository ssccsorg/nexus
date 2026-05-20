// nexus-table — Utility functions and shared types.

/// Lightweight project metadata matching Cairn's Project schema.
#[derive(Debug, Clone)]
pub struct ProjectMeta {
    pub id: String,
    pub title: String,
    pub status: String,
    pub created_at: String,
    pub reason_worker: Option<String>,
    pub reason_trigger: Option<String>,
    pub reason_started_at: Option<String>,
    pub reason_last_heartbeat_at: Option<String>,
}
