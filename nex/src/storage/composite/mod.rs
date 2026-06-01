pub mod cold;
pub mod iobuf;
pub mod session_server;
pub mod store_session;

pub use cold::CompositeColdStorage;
pub use iobuf::{IoBufferBlob, IoBufferKv, IoBufferObject, IoBufferSessionMeta};
pub use session_server::{SessionHandle, SessionServer};
pub use store_session::IoBufferSession;

/// Blob key prefix listing all flush blobs for a project+entity+partition.
pub(super) fn flush_blob_prefix(project_id: &str, entity: &str, partition: &str) -> String {
    format!("{project_id}/flush/{entity}/{partition}/")
}
