// ── nex-io: re-export shim ─────────────────────────────────────────────
//
// The pure IO abstraction layer has moved to chton (io module) as part of
// the IO layer separation: IO concepts belong to chton, the IO
// materialization layer. This crate remains as a re-export shim so
// existing consumers (nex, apps) compile unchanged.

#![no_std]

pub use chton::io::*;
