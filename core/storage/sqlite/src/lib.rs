// nexus-storage-sqlite — SQLite-backed ColdStorage implementations.
//
// Storage layer only — no graph dependency. Implements the capability-based
// Storage traits (`StorageRead`, `FactCapable`, `IntentCapable`, `HintCapable`,
// `FilterCapable`, `FihPersistence`, `ColdStorage`) from nexus-model.

pub mod schema;
pub mod sql_legacy;
pub mod sql_normalized;
pub mod util;

pub use sql_legacy::SqliteStorage;
pub use sql_normalized::SqlNormalizedStorage;
pub use util::ProjectMeta;

// Re-export key types from nexus-model that consumers of this crate commonly need.
pub use nexus_model::{
    BlackboardError, BoardState, ColdStorage, Fact, FihHash, FihPersistence, Hint, Intent,
    StateFilter, StorageRead,
};
