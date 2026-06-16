// ── IO abstraction layer ──────────────────────────────────────────────
//
// Pure IO interface for the nexus runtime. Defines the AsyncFileIo trait
// that storage engines use to read/write/list/delete data.
//
// This layer is storage-agnostic. Multiple storage engines (FihStorage,
// PetgraphStorage, etc.) can share the same IO interface.

pub mod async_file_io;
pub mod sim_io;

pub use async_file_io::{AsyncFileIo, IoFuture, SyncFileIo, WriteOp};
pub use sim_io::SimIo;
