// ── Error type ───────────────────────────────────────────────────────────

use alloc::string::String;

#[derive(Debug, Clone)]
pub enum BlackboardError {
    NotFound(String),
    Conflict(String),
    Forbidden(String),
    BadRequest(String),
    Internal(String),
}

impl core::fmt::Display for BlackboardError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotFound(m) => write!(f, "not found: {m}"),
            Self::Conflict(m) => write!(f, "conflict: {m}"),
            Self::Forbidden(m) => write!(f, "forbidden: {m}"),
            Self::BadRequest(m) => write!(f, "bad request: {m}"),
            Self::Internal(m) => write!(f, "internal: {m}"),
        }
    }
}

impl core::error::Error for BlackboardError {}
