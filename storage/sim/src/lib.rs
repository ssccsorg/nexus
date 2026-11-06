// ── nexus-storage-sim: FIH storage simulator ─────────────────────────
//
// Simulator (test double) layer. Provides in-memory IO backend (SimIo)
// that implements the FileIo trait defined in nex::io.
//
// The reference node supplies the implementation types, the model supplies the primitives,
// and this crate supplies the in-memory medium. The filesystem IO backend lives in nex-io.

pub mod sim_io;

pub use nex::io::*;
pub use nex_fih::core::*;
pub use nexus_storage_ref::*;
pub use sim_io::SimIo;
