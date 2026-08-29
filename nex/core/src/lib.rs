// nex-core — Foundation storage interfaces with zero knowledge of FIH types.
//
// This crate contains ONLY traits that have no dependency on Fact, Intent, Hint,
// BoardState, or any other FIH type. Every trait here is pure — it stands on its
// own and can be implemented by any backend regardless of FIH semantics.
//
// For traits that depend on StorageRead (which returns BoardState), see nex-fih.

pub mod clock;
pub mod storage;

pub use clock::{Now, OffsetClock, SystemClock};
pub use storage::blob_store::BlobStore;
pub use storage::meta_store::MetaStore;
pub use storage::object_store::ObjectStore;
