// ── IO layer: FIH storage over AsyncFileIo ────────────────────────────
//
// Built-in core storage for the nexus runtime. Provides the AsyncFileIo
// abstraction, FihStorage engine, EntityStore, records, index,
// FihSession lifecycle, and SystemClock.
//
// Architecture: built-in storage (Mac mini) with external storage slots
//   nex::storage::io = built-in core storage
//   nexus-storage-sim = simulator / IO backends
//   storage/composite, storage/petgraph = external storage (future)

pub mod async_file_io;
pub mod clock;
pub mod entity_store;
pub mod export;
pub mod index;
pub mod intent_status;
pub mod record;
pub mod session;
pub mod sim_io;
pub mod store;

pub use async_file_io::{AsyncFileIo, IoFuture, SyncFileIo, WriteOp};
pub use clock::SystemClock;
pub use entity_store::{EntityStore, MemoryEntityStore};
pub use export::{FihExport, FihImport, export_from_io, import_into_io};
pub use index::OrderedIndex;
pub use intent_status::IntentStatus;
pub use record::{ContentMeta, FactRecord, HintRecord, IntentRecord};
pub use session::FihSession;
pub use sim_io::SimIo;
pub use store::FihStorage;
