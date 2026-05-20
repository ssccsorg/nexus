/// Cold storage backends for nexus-graph.
///
/// Each module provides a durable [`ColdStorage`] implementation backed
/// by SQLite. Two variants exist:
///
/// - `sql_normalized` — Normalized Cairn-pattern tables (facts, intents,
///   hints, intent_sources). Project-scoped. Recommended for production.
///
/// - `sql_legacy` — Legacy event-log table (`fih_events`). Retained for
///   backward compatibility with existing databases. New code should
///   prefer `sql_normalized`.

pub mod sql_legacy;
pub mod sql_normalized;

pub use sql_legacy::SqliteStorage;
pub use sql_normalized::SqlNormalizedStorage;
