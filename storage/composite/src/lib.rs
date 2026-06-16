mod async_store;
pub mod blackboard;
mod cold;
mod session_server;
mod store_session;

pub use async_store::{AsyncStoreBlob, AsyncStoreKv, AsyncStoreObject, AsyncStoreSessionMeta};
pub use blackboard::CompositeBlackboard;
pub use cold::CompositeColdStorage;
pub use session_server::{SessionHandle, SessionServer};
pub use store_session::AsyncStoreSession;

#[allow(dead_code)]
pub use cold::{flush_snapshot_to_blob, load_latest_snapshot};

/// Blob key prefix listing all flush blobs for a project+entity+partition.
pub fn flush_blob_prefix(project_id: &str, entity: &str, partition: &str) -> String {
    format!("{project_id}/flush/{entity}/{partition}/")
}
