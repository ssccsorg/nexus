// nexus-table — Utility functions and shared types.

use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

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

pub fn utc_now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00.000000Z".into())
}
