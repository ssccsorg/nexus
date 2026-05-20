// nexus-table — Utility functions and shared types.

use std::time::{SystemTime, UNIX_EPOCH};

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
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();
    let micros = d.subsec_micros();

    // Compute date from days since epoch using a compact civil-date algorithm.
    let z = secs / 86400 + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    let h = (secs % 86400) / 3600;
    let mi = (secs % 3600) / 60;
    let s = secs % 60;

    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}Z", y, m, d, h, mi, s, micros)
}
