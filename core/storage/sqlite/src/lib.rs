// nexus-storage-sqlite — SQLite-backed ColdStorage implementations.
//
// Storage layer only — no graph dependency. Implements the `Storage`
// + `ColdStorage` traits from nexus-model.

pub mod schema;
pub mod sql_legacy;
pub mod sql_normalized;
pub mod util;

pub use sql_legacy::SqliteStorage;
pub use sql_normalized::SqlNormalizedStorage;
pub use util::ProjectMeta;
