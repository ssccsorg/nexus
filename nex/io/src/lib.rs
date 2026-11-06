// ── nex-io: re-export shim ─────────────────────────────────────────────
//
// The pure IO abstraction layer has moved to chton (io module) as part of
// the IO layer separation: IO concepts belong to chton, the IO
// materialization layer. This crate remains as a re-export shim so
// existing consumers (nex, apps) compile unchanged.

#![no_std]
extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "std")]
pub mod memory;

pub use chton::io::*;
#[cfg(feature = "std")]
pub use memory::{MemoryIo, SimIo};
