pub mod async_store;
pub mod cold;
pub mod session_server;
pub mod store_session;

pub use async_store::{AsyncStoreBlob, AsyncStoreKv, AsyncStoreObject, AsyncStoreSessionMeta};
pub use cold::CompositeColdStorage;
pub use session_server::{SessionHandle, SessionServer};
pub use store_session::AsyncStoreSession;

/// Blob key prefix listing all flush blobs for a project+entity+partition.
pub(super) fn flush_blob_prefix(project_id: &str, entity: &str, partition: &str) -> String {
    format!("{project_id}/flush/{entity}/{partition}/")
}
