// ── Core primitives ─────────────────────────────────────────────────────
//
// The record format, the intent lifecycle, the ordered index, the session and blackboard
// wrappers, and the IO re-exports. The reference store is not here: it is `crate::reference`,
// because a store is not a primitive of the model.

#[cfg(feature = "std")]
pub mod export;
pub mod fih_blackboard;
pub mod index;
pub mod intent_status;
pub mod record;
pub mod session;

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
