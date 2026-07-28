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
/// (Available on wasm32-wasip2 where std::fs is present.)
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub mod fs_io;

pub use file_io::{BatchIo, FileIo, IoFuture, SyncFileIo, WriteOp, default_apply_batch};
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub use fs_io::FsIo;
