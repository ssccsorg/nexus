// ── nexus-storage-sim: Native FIH storage prototype ─────────────────────
//
// Unified storage interface over FihIo. Replaces DualStorage + CompositeColdStorage
// with a single storage backed by an abstract IO layer.
//
// All FIH trait implementations are sync. IO is enqueued as WriteOps in a
// buffer and flushed by the outer FihSession layer (see session.rs).

/// Filesystem-backed IO. Gated to non-wasm32 targets because walkdir and
/// std::fs directory traversal are not available on wasm32-unknown-unknown.
#[cfg(not(target_arch = "wasm32"))]
pub mod fs_io;
pub mod index;
pub mod intent_status;
pub mod io;
pub mod record;
pub mod session;
pub mod sim_io;
pub mod store;

pub use io::{AsyncFihIo, BlockingFihIo, WriteOp};
pub use sim_io::SimFihIo;
pub use store::NativeFihStorage;
