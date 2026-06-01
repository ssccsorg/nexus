// nexus-storage-composite — Platform-independent multi-tier cold storage for FIH.
//
// Provides CompositeColdStorage and IoBuffer implementations.
//
// Protocol traits (BlobStore, ObjectStore, MetaStore, Now, SystemClock) are
// defined in nexus-model. This crate provides concrete implementations
// (IoBufferBlob, IoBufferObject, IoBufferKv, IoBufferSessionMeta) and
// the CompositeColdStorage orchestration layer.

pub mod composite;
pub mod iobuf;
pub mod session_server;
pub mod store_session;

pub use composite::CompositeColdStorage;
pub use iobuf::{IoBufferBlob, IoBufferKv, IoBufferObject, IoBufferSessionMeta};
pub use session_server::{SessionHandle, SessionServer};
pub use store_session::IoBufferSession;

// ── Internal key conventions ─────────────────────────────────────────────

/// Blob key prefix listing all flush blobs for a project+entity+partition.
pub(crate) fn flush_blob_prefix(project_id: &str, entity: &str, partition: &str) -> String {
    format!("{project_id}/flush/{entity}/{partition}/")
}
