use super::evict::EvictCapable;
use super::fact::FactCapable;
use super::flush::FlushCapable;
use super::hint::HintCapable;
use super::intent::IntentCapable;
use super::scan::ScanCapable;
use super::time_range::TimeRangeCapable;

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

/// Full FIH persistence: what a Blackboard backend must provide.
pub trait FihPersistence: FactCapable + IntentCapable + HintCapable {}
impl<T: FactCapable + IntentCapable + HintCapable> FihPersistence for T {}

/// Cold storage: durable persistence — scan, flush, evict, time range.
///
/// Provides write_blob() so the flush coordinator can write hot data
/// to cold blob before advancing the cursor.
pub trait ColdStorage:
    ScanCapable + TimeRangeCapable + FlushCapable + EvictCapable + StorageSend
{
    /// Write raw bytes to a blob key. Used by the flush coordinator.
    fn write_blob(&self, key: &str, data: &[u8]) -> Result<(), String>;
}
