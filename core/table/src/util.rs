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

pub fn utc_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();

    let secs_per_day: i64 = 86400;
    let mut days = (secs / secs_per_day as u64) as i64;
    let day_secs = (secs % secs_per_day as u64) as i64;
    let h = day_secs / 3600;
    let m = (day_secs % 3600) / 60;
    let s = day_secs % 60;

    let mut y = 1970i64;
    loop {
        let days_in_year = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 {
            366
        } else {
            365
        };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let month_days: [i64; 12] = if leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut mo = 1u32;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        mo += 1;
    }
    let day = days + 1;

    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, day, h, m, s)
}
