// ── nexus-storage-ref: the reference node ──────────────────────────────
//
// The reference implementation of the storage contract, and the reader that shows the key
// space. It holds FIH records in maps, writes them at the keys the format publishes, and
// answers the read surface. It carries no index over the axes, no paged walk, no content
// cache, and no recovery, because those are an implementation's decisions.
//
// This is a node, not a layer of the fabric: a consumer reaches it through the network, and
// the network links nothing here.

#![no_std]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

pub mod blackboard;
#[cfg(feature = "std")]
pub mod export;
pub mod intent_status;
pub mod record;
#[cfg(feature = "std")]
pub mod session;
pub mod store;

pub use blackboard::FihBlackboard;
#[cfg(feature = "std")]
pub use export::{export_from_io, import_into_io};
pub use intent_status::IntentStatus;
pub use record::{ContentMeta, FactRecord, HintRecord, IntentRecord, RecordTimeUnit};
#[cfg(feature = "std")]
pub use session::FihSession;
pub use store::{Record, RefFihStorage};
