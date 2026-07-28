// ── IO re-exports from nex-io ─────────────────────────────────────────
//
// This module re-exports IO types from the nex-io crate so that
// FIH implementation files (core/store.rs, core/export.rs, etc.)
// can use `crate::io::file_io::FileIo` paths without depending on
// the full crate path directly.
//
// All IO types are defined in the nex-io crate; this module is a
// compatibility shim.

pub use nex_io::{BatchIo, FileIo, IoFuture, SyncFileIo, WriteOp, default_apply_batch};
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub use nex_io::FsIo;

/// Module alias so that `crate::io::file_io::FileIo` resolves.
pub mod file_io {
    pub use nex_io::{BatchIo, FileIo, IoFuture, SyncFileIo, WriteOp, default_apply_batch};
}

/// Module alias so that `crate::io::fs_io::FsIo` resolves.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub mod fs_io {
    pub use nex_io::FsIo;
}
