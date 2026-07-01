use std::fmt;
use std::io;

#[derive(Debug)]
pub enum Error {
    Io { message: String, source: io::Error },
    Timeout { operation: String, timeout_ms: u64 },
    InvalidConfig(String),
    InvalidState(String),
    Signal(String),
    Runtime(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { message, source } => write!(f, "I/O error: {message}: {source}"),
            Self::Timeout { operation, timeout_ms } => write!(f, "{operation} timed out after {timeout_ms}ms"),
            Self::InvalidConfig(msg) => write!(f, "Invalid configuration: {msg}"),
            Self::InvalidState(msg) => write!(f, "Invalid state: {msg}"),
            Self::Signal(msg) => write!(f, "Signal error: {msg}"),
            Self::Runtime(msg) => write!(f, "Runtime error: {msg}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl Error {
    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::Timeout { .. })
    }
    pub fn invalid_config(msg: impl Into<String>) -> Self {
        Self::InvalidConfig(msg.into())
    }
    pub fn invalid_state(msg: impl Into<String>) -> Self {
        Self::InvalidState(msg.into())
    }
    pub fn signal(msg: impl Into<String>) -> Self {
        Self::Signal(msg.into())
    }
    pub fn runtime(msg: impl Into<String>) -> Self {
        Self::Runtime(msg.into())
    }
    pub fn io(msg: impl Into<String>, source: io::Error) -> Self {
        Self::Io { message: msg.into(), source }
    }
    pub fn timeout(operation: impl Into<String>, timeout_ms: u64) -> Self {
        Self::Timeout { operation: operation.into(), timeout_ms }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io { message: e.to_string(), source: e }
    }
}
