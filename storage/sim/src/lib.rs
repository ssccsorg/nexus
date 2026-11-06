// ── nexus-storage-sim: FIH storage simulator ─────────────────────────
//
// Simulator (test double) layer. Provides in-memory IO backend (SimIo)
// that implements the FileIo trait defined in nex::io.
//
// All core storage types (RefFihStorage, EntityStore, etc.) are
// re-exported from nex-fih. The filesystem IO backend (FsIo)
// now lives in nex::io::fs_io.

pub mod sim_io;

// Re-export core storage types from nex for backward compatibility.
pub use nex::io::*;
pub use nex_fih::core::*;
pub use nex_fih::{Record, RefFihStorage};
pub use sim_io::SimIo;
