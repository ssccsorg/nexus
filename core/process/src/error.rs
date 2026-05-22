// nexus-process — Error types for the process layer.

use std::fmt;

/// Errors that can occur during OODA loop execution.
#[derive(Debug)]
pub enum ProcessError {
    /// The blackboard rejected an operation (e.g. duplicate Intent, not found).
    Blackboard(String),
    /// Eviction backend failed.
    Eviction(String),
    /// Snapshot persistence failed.
    Snapshot(String),
    /// Internal process error.
    Internal(String),
}

impl fmt::Display for ProcessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Blackboard(msg) => write!(f, "blackboard: {msg}"),
            Self::Eviction(msg) => write!(f, "eviction: {msg}"),
            Self::Snapshot(msg) => write!(f, "snapshot: {msg}"),
            Self::Internal(msg) => write!(f, "internal: {msg}"),
        }
    }
}

impl std::error::Error for ProcessError {}

impl From<String> for ProcessError {
    fn from(msg: String) -> Self {
        Self::Internal(msg)
    }
}
