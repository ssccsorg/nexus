// ── nexus-storage-sim: FIH storage simulator ─────────────────────────
//
// Simulator (test double) layer. Provides IO backends (SimIo, FsIo)
// that implement the AsyncFileIo trait defined in nex::io.
//
// All core storage types (FihStorage, EntityStore, etc.) are
// re-exported from nex::storage::core.

/// Filesystem-backed IO. Gated to non-wasm32 targets.
#[cfg(not(target_arch = "wasm32"))]
pub mod fs_io;
pub mod sim_io;

// Re-export core storage types from nex for backward compatibility.
pub use nex::io::*;
pub use nex::storage::core::*;
pub use sim_io::SimIo;
