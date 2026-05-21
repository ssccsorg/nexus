use super::evict::EvictCapable;
use super::fact::FactCapable;
use super::filter::FilterCapable;
use super::flush::FlushCapable;
use super::hint::HintCapable;
use super::intent::IntentCapable;
use super::scan::ScanCapable;
use super::time_range::TimeRangeCapable;

/// Full FIH persistence: what a Blackboard backend must provide.
pub trait FihPersistence: FactCapable + IntentCapable + HintCapable {}
impl<T: FactCapable + IntentCapable + HintCapable> FihPersistence for T {}

/// Hot storage: full FIH + memory management + time range (petgraph).
pub trait HotStorage: FihPersistence + EvictCapable + TimeRangeCapable {}
impl<T: FihPersistence + EvictCapable + TimeRangeCapable> HotStorage for T {}

/// Cold storage: full FIH + filtered reads + scan + time range + flush (SQLite, Parquet).
pub trait ColdStorage:
    FihPersistence + FilterCapable + ScanCapable + TimeRangeCapable + FlushCapable
{
}
impl<T: FihPersistence + FilterCapable + ScanCapable + TimeRangeCapable + FlushCapable> ColdStorage
    for T
{
}
