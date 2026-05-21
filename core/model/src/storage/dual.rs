use super::aggregate::{ColdStorage, HotStorage};
use super::evict::EvictCapable;
use super::fact::FactCapable;
use super::filter::{FilterCapable, StateFilter};
use super::flush::{FlushCapable, FlushCursor, FlushResult};
use super::hint::HintCapable;
use super::intent::IntentCapable;
use super::read::StorageRead;
use super::scan::PartitionData;
use super::scan::ScanCapable;
use super::time_range::TimeRangeCapable;
use crate::error::BlackboardError;
use crate::fih::{BoardState, Fact, FihHash, Hint, Intent};
use std::ops::Range;

/// Composes a Hot + Cold storage pair.
///
/// - Writes go to both hot and cold (dual-write for durability).
/// - Reads go to hot (early return, edge computing fast path).
/// - Flush/evict delegate to the appropriate layer.
pub struct DualStorage {
    hot: Box<dyn HotStorage>,
    cold: Box<dyn ColdStorage>,
}

impl DualStorage {
    pub fn new(hot: Box<dyn HotStorage>, cold: Box<dyn ColdStorage>) -> Self {
        Self { hot, cold }
    }

    pub fn hot(&self) -> &dyn HotStorage {
        &*self.hot
    }

    pub fn cold(&self) -> &dyn ColdStorage {
        &*self.cold
    }
}

// ── Core read ──

impl StorageRead for DualStorage {
    fn project_id(&self) -> &str {
        self.hot.project_id()
    }

    fn read_state(&self) -> BoardState {
        self.hot.read_state()
    }
}

// ── FIH writes: delegate to both hot + cold ──

impl FactCapable for DualStorage {
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        let hash = self.hot.submit_fact(fact)?;
        let _ = self.cold.submit_fact(fact);
        Ok(hash)
    }
}

impl IntentCapable for DualStorage {
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        let hash = self.hot.submit_intent(intent)?;
        let _ = self.cold.submit_intent(intent);
        Ok(hash)
    }

    fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.hot.claim_intent(intent_id, agent)?;
        let _ = self.cold.claim_intent(intent_id, agent);
        Ok(())
    }

    fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.hot.heartbeat(intent_id, agent)?;
        let _ = self.cold.heartbeat(intent_id, agent);
        Ok(())
    }

    fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.hot.release_intent(intent_id, agent)?;
        let _ = self.cold.release_intent(intent_id, agent);
        Ok(())
    }

    fn conclude_intent(
        &self,
        intent_id: &str,
        result: &serde_json::Value,
    ) -> Result<Fact, BlackboardError> {
        let fact = self.hot.conclude_intent(intent_id, result)?;
        let _ = self.cold.conclude_intent(intent_id, result);
        Ok(fact)
    }
}

impl HintCapable for DualStorage {
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        self.hot.submit_hint(hint)?;
        let _ = self.cold.submit_hint(hint);
        Ok(())
    }
}

// ── Filtered reads: delegate to cold (hot typically doesn't support filtering) ──

impl FilterCapable for DualStorage {
    fn read_state_filtered(&self, filter: &StateFilter) -> BoardState {
        self.cold.read_state_filtered(filter)
    }
}

// ── Memory management: delegate to hot ──

impl EvictCapable for DualStorage {
    fn approximate_size(&self) -> usize {
        self.hot.approximate_size()
    }

    fn evict_before(&self, before: &str) -> Result<u64, String> {
        self.hot.evict_before(before)
    }
}

// ── Partition scan: delegate to cold ──

impl ScanCapable for DualStorage {
    fn scan_partition(&self, partition: &str) -> Result<PartitionData, String> {
        self.cold.scan_partition(partition)
    }
}

// ── Time range: merge hot and cold ranges ──

impl TimeRangeCapable for DualStorage {
    fn time_range(&self) -> Option<Range<String>> {
        let hot_range = self.hot.time_range();
        let cold_range = self.cold.time_range();
        match (hot_range, cold_range) {
            (Some(h), Some(c)) => {
                let start = std::cmp::min(h.start, c.start);
                let end = std::cmp::max(h.end, c.end);
                Some(start..end)
            }
            (Some(h), None) => Some(h),
            (None, Some(c)) => Some(c),
            (None, None) => None,
        }
    }
}

// ── Flush: delegate to cold (cold is the durable target) ──

impl FlushCapable for DualStorage {
    fn flush_since(&self, cursor: &FlushCursor) -> Result<FlushResult, String> {
        self.cold.flush_since(cursor)
    }
}
