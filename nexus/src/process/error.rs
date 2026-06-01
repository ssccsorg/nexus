// ── Process-level errors ───────────────────────────────────────────────────

use std::fmt;

/// Errors originating from the OODA loop runtime (process crate).
#[derive(Debug, Clone)]
pub enum ProcessError {
    /// Blackboard operation failed.
    Blackboard(String),
    /// Memory eviction failed.
    Eviction(String),
    /// Snapshot serialization/deserialization failed.
    Snapshot(String),
    /// Internal process error.
    Internal(String),
}

impl fmt::Display for ProcessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Blackboard(msg) => write!(f, "blackboard error: {msg}"),
            Self::Eviction(msg) => write!(f, "eviction error: {msg}"),
            Self::Snapshot(msg) => write!(f, "snapshot error: {msg}"),
            Self::Internal(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

impl std::error::Error for ProcessError {}

impl From<nexus_model::BlackboardError> for ProcessError {
    fn from(e: nexus_model::BlackboardError) -> Self {
        Self::Blackboard(e.to_string())
    }
}
