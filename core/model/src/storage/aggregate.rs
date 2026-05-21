use super::evict::EvictCapable;
use super::fact::FactCapable;
use super::filter::FilterCapable;
use super::hint::HintCapable;
use super::intent::IntentCapable;

/// Full FIH persistence: what a Blackboard backend must provide.
pub trait FihPersistence: FactCapable + IntentCapable + HintCapable {}
impl<T: FactCapable + IntentCapable + HintCapable> FihPersistence for T {}

/// Hot storage: full FIH + memory management (petgraph).
pub trait HotStorage: FihPersistence + EvictCapable {}
impl<T: FihPersistence + EvictCapable> HotStorage for T {}

/// Cold storage: full FIH + filtered reads (SQLite, Parquet).
pub trait ColdStorage: FihPersistence + FilterCapable {}
impl<T: FihPersistence + FilterCapable> ColdStorage for T {}
