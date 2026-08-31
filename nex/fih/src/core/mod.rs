// ── Core storage engine: FihStorage ───────────────────────────────────
//
// Built-in storage engine for the nexus runtime. Implements the full FIH
// lifecycle (Fact, Intent, Hint) on top of the IO abstraction layer.
//
// Uses crate::io::FileIo for all IO operations.
// Can be swapped out for external storage engines.

pub mod export;
pub mod fih_blackboard;
pub mod index;
pub mod intent_status;
pub mod record;
pub mod session;
pub mod store;
pub mod structural;

// Store surface ownership: chton is the behavior layer. The EntityStore
// family (trait + memory + materialized impls) lives in chton::store and
// is re-exported here so nexus consumers keep a stable crate path.
pub use chton::store::{CoordEntityStore, EntityStore, MapEntityStore, MemoryEntityStore};
#[cfg(feature = "std")]
pub use export::{export_from_io, import_into_io};
#[cfg(feature = "std")]
pub use fih_blackboard::FihBlackboard;
pub use index::{Cell2, OrderedIndex};
pub use intent_status::IntentStatus;
pub use record::{ContentMeta, FactRecord, HintRecord, IntentRecord};
#[cfg(feature = "std")]
pub use session::FihSession;
pub use store::FihStorage;
