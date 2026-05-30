use super::cypher::CypherCapable;
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

/// Hot storage: full FIH + memory management + time range + Cypher + filter (petgraph).
pub trait HotStorage:
    FihPersistence + FilterCapable + CypherCapable + EvictCapable + TimeRangeCapable
{
}
impl<T: FihPersistence + FilterCapable + CypherCapable + EvictCapable + TimeRangeCapable> HotStorage
    for T
{
}

/// Cold storage: durable persistence — scan, flush, evict, time range, Cypher query.
///
/// Does NOT include FihPersistence or StorageRead — graph CRUD is handled by
/// HotStorage (Petgraph). ColdStorage only manages blob archives, CAS coordination,
/// and metadata (cursor, snapshot pointers).
pub trait ColdStorage:
    ScanCapable + TimeRangeCapable + FlushCapable + CypherCapable + EvictCapable
{
}
impl<T: ScanCapable + TimeRangeCapable + FlushCapable + CypherCapable + EvictCapable> ColdStorage
    for T
{
}
