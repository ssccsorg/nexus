#![allow(clippy::collapsible_if, clippy::items_after_statements)]

// ── nexd library crate ──────────────────────────────────────────────────
//
// Re-exports public API for integration testing and embedding.

pub mod config;
pub mod handler;
pub mod manager;
pub mod server;

pub use config::NexdConfig;
pub use manager::ProcessManager;
pub mod daemon;
