/// Blob store for binary data (snapshots, flush archives, Parquet chunks).
///
/// Implementations: AsyncStoreBlob (in-memory HashMap),
/// R2 bucket (CF Workers), local filesystem (server).
pub trait BlobStore: Send + Sync {
    /// Store binary data at the given key.
    fn put(&self, key: &str, data: &[u8]) -> Result<(), String>;

    /// Retrieve binary data by key. Returns None if not found.
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, String>;

    /// Delete a blob. Ok if key does not exist.
    fn delete(&self, key: &str) -> Result<(), String>;

    /// List all blob keys with the given prefix.
    fn list(&self, prefix: &str) -> Result<Vec<String>, String>;
}
