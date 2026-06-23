// ── Core storage engine: FihStorage ───────────────────────────────────
//
// Built-in storage engine for the nexus runtime. Implements the full FIH
// lifecycle (Fact, Intent, Hint) on top of the IO abstraction layer.
//
// Uses crate::io::AsyncFileIo for all IO operations.
// Can be swapped out for external storage engines.

pub mod entity_store;
pub mod export;
pub mod index;
pub mod intent_status;
pub mod record;
pub mod session;
pub mod store;

pub use entity_store::{EntityStore, MemoryEntityStore};
pub use export::{FihExport, FihImport, export_from_io, import_into_io};
pub use index::{Cell2, OrderedIndex};
pub use intent_status::IntentStatus;
pub use record::{ContentMeta, FactRecord, HintRecord, IntentRecord};
pub use session::FihSession;
pub use store::{ChainEntry, FihStorage};
