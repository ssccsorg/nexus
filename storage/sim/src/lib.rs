// ── nexus-storage-sim: FIH storage simulator ─────────────────────────
//
// Simulator (test double) layer. Provides in-memory IO backend (SimIo)
// that implements the FileIo trait defined in nex::io.
//
// All core storage types (FihStorage, EntityStore, etc.) are
// re-exported from nex::storage::core. The filesystem IO backend (FsIo)
// now lives in nex::io::fs_io.

pub mod sim_io;

// Re-export core storage types from nex for backward compatibility.
pub use nex::io::*;
pub use nex::storage::core::*;
pub use sim_io::SimIo;
