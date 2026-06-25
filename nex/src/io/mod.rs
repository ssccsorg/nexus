// ── IO abstraction layer ──────────────────────────────────────────────
//
// Pure IO interface for the nexus runtime. Defines the FileIo trait
// that IO backends implement for read/write/list/delete operations,
// and the BatchIo lego trait for backends that support atomic batch commits.
//
// IO backends implement this trait. Higher layers (storage engines, etc.)
// consume it without the IO layer knowing about them.

pub mod file_io;
/// Filesystem-backed IO. Not available on wasm32-unknown-unknown.
#[cfg(not(target_arch = "wasm32"))]
pub mod fs_io;

pub use file_io::{BatchIo, FileIo, IoFuture, SyncFileIo, WriteOp, default_apply_batch};
#[cfg(not(target_arch = "wasm32"))]
pub use fs_io::FsIo;
