// nexus-table — SQLite-backed FIH storage.
//
// # Crate structure
//
// - `storage` — Legacy event-log storage (SqliteStorage)
// - `blackboard` — Normalized Cairn-pattern Blackboard (SqlBlackboard)
// - `schema` — Database DDL (apply_schema)
// - `util` — Shared types and utilities (ProjectMeta)

pub mod blackboard;
pub mod schema;
pub mod storage;
pub mod util;

pub use blackboard::SqlBlackboard;
pub use nexus_model::{
    Blackboard, BlackboardError, BoardState, Fact, FihHash, Hint, Intent, Storage, StoredEvent,
};
pub use storage::SqliteStorage;
pub use util::ProjectMeta;
