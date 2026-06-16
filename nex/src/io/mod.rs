// ── IO abstraction layer ──────────────────────────────────────────────
//
// Pure IO interface for the nexus runtime. Defines the AsyncFileIo trait
// that storage engines use to read/write/list/delete data.
//
// This layer is storage-agnostic. Multiple storage engines (FihStorage,
// PetgraphStorage, etc.) can share the same IO interface.
//
// IO implementations (SimIo, CfFihIo, FsIo) live in the nexus-storage-sim
// crate. This crate defines only the trait/type definitions.

pub mod async_file_io;

pub use async_file_io::{AsyncFileIo, IoFuture, SyncFileIo, WriteOp};
