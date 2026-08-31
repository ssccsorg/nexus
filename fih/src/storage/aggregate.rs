use super::evict::EvictCapable;
use super::scan::ScanCapable;
use super::time_range::TimeRangeCapable;
use alloc::string::String;

#[cfg(not(target_arch = "wasm32"))]
pub mod send_marker {
    /// Marker trait alias for Send on native targets.
    /// On WASM, types do not need Send because WASM is single-threaded.
    pub trait StorageSend: Send {}
    impl<T: Send> StorageSend for T {}
}

#[cfg(target_arch = "wasm32")]
pub mod send_marker {
    /// On WASM, Send is not required. Blanket impl for any type.
    pub trait StorageSend {}
    impl<T> StorageSend for T {}
}

pub use send_marker::StorageSend;

/// Cold storage: durable persistence — scan, evict, time range.
///
/// Provides write_blob() for raw byte writes to the durable medium.
pub trait ColdStorage: ScanCapable + TimeRangeCapable + EvictCapable + StorageSend {
    /// Write raw bytes to a blob key.
    fn write_blob(&self, key: &str, data: &[u8]) -> Result<(), String>;
}
