// nexus-model — Re-export shim for nex-core + nex-fih.
//
// This crate exists for backward compatibility. All types and traits are
// now defined in nex-core (pure storage interfaces) and nex-fih (FIH
// types and storage traits).
//
// New code should import directly from nex-core and nex-fih.
// This shim will be removed after the transition period.

pub use nex_core::*;
pub use nex_fih::*;

// Re-export storage module so `nexus_model::storage::GraphRead` paths work.
pub mod storage {
    pub use nex_fih::storage::*;
}

// ── Deprecated: kept only for existing consumers ─────────────────────────

#[allow(deprecated)]
mod interner;
#[allow(deprecated)]
pub use interner::Interner;
