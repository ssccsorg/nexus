use super::evict::EvictCapable;
use super::fact::FactCapable;
use super::filter::FilterCapable;
use super::flush::FlushCapable;
use super::governance::GovernanceCapable;
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

/// Full FIH persistence with governance: what a governed Blackboard backend must provide.
pub trait GovernedPersistence: FihPersistence + GovernanceCapable {}
impl<T: FihPersistence + GovernanceCapable> GovernedPersistence for T {}

/// Delta set of postcard-serialized entity blobs: (facts, intents, hints).
pub type DeltaSet = (Vec<Vec<u8>>, Vec<Vec<u8>>, Vec<Vec<u8>>);

/// Hot storage: full FIH + memory management + time range + filter (petgraph).
pub trait HotStorage:
    FihPersistence + FilterCapable + EvictCapable + TimeRangeCapable + StorageSend
{
    /// Read all entities submitted after a given cursor timestamp.
    /// Returns (fact_bytes, intent_bytes, hint_bytes) as postcard-serialized blobs.
    fn read_delta_since(&self, _cursor_ts: &str) -> DeltaSet {
        (Vec::new(), Vec::new(), Vec::new())
    }
}

/// Cold storage: durable persistence — scan, flush, evict, time range, Cypher query.
///
/// Does NOT include FihPersistence or StorageRead — graph CRUD is handled by
/// HotStorage (Petgraph). ColdStorage only manages blob archives, CAS coordination,
/// and metadata (cursor, snapshot pointers).
///
/// Provides write_blob() so DualStorage flush coordinator can write hot data
/// to cold blob before advancing the cursor.
pub trait ColdStorage:
    ScanCapable + TimeRangeCapable + FlushCapable + EvictCapable + StorageSend
{
    /// Write raw bytes to a blob key. Used by the flush coordinator.
    fn write_blob(&self, key: &str, data: &[u8]) -> Result<(), String>;
}
