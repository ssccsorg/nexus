// ── nexus-storage-sim: FIH storage simulator ─────────────────────────
//
// Simulator (test double) layer. Provides in-memory IO backend (SimIo)
// that implements the FileIo trait defined in nex::io.
//
// The reference node supplies the implementation types, the model supplies the primitives,
// and nex-io supplies the media, in memory and on a file system. This crate supplies the
// scenarios that drive them.

pub use nex::io::*;
pub use nex_fih::core::*;
pub use nexus_storage_ref::*;
pub use nex_io::{MemoryIo, SimIo};
