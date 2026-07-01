// ── daemon — Built-in daemon runtime (adapted from proc-daemon) ────────
//
// Provides graceful shutdown, signal handling, and concurrent task management.
// Embedded copy of patterns from proc-daemon (Apache 2.0).

pub mod daemon_config;
mod daemon_core;
pub mod error;
pub mod shutdown;
pub mod signal;


pub use daemon_config::{Config, LogLevel};
pub use daemon_core::Daemon;
pub use error::{Error, Result};
pub use shutdown::{ShutdownHandle, ShutdownReason};
