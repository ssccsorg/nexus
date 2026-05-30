// DualStorage — composes a Hot (Petgraph) + Cold (Composite) storage pair.

use super::aggregate::{ColdStorage, HotStorage};
use super::cypher::CypherCapable;
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
/// - Writes go ONLY to hot (Petgraph). Cold does NOT store graph data.
/// - Reads go to hot (early return, edge computing fast path).
/// - Flush/evict delegate to cold for durable persistence.
pub struct DualStorage {
    hot: Box<dyn HotStorage>,
    cold: Box<dyn ColdStorage>,
}

impl DualStorage {
    /// Create a new DualStorage pair.
    ///
    /// Panics if hot and cold have different project_ids.
    /// project_id must be issued from a single source.
    pub fn new(hot: Box<dyn HotStorage>, cold: Box<dyn ColdStorage>) -> Self {
        assert_eq!(
            hot.project_id(),
            cold.project_id(),
            "DualStorage: hot and cold must share the same project_id (hot={}, cold={})",
            hot.project_id(),
            cold.project_id()
        );
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

// ── FIH writes: delegate to hot ONLY (cold no longer stores graph data) ──

impl FactCapable for DualStorage {
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        self.hot.submit_fact(fact)
    }
}

impl IntentCapable for DualStorage {
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        self.hot.submit_intent(intent)
    }

    fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.hot.claim_intent(intent_id, agent)
    }

    fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.hot.heartbeat(intent_id, agent)
    }

    fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError> {
        self.hot.release_intent(intent_id, agent)
    }

    fn conclude_intent(
        &self,
        intent_id: &str,
        result: &str,
    ) -> Result<Fact, BlackboardError> {
        self.hot.conclude_intent(intent_id, result)
    }
}

impl HintCapable for DualStorage {
    fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError> {
        self.hot.submit_hint(hint)
    }
}

// ── Filtered reads: delegate to hot (Petgraph implements FilterCapable) ──

impl FilterCapable for DualStorage {
    fn read_state_filtered(&self, filter: &StateFilter) -> BoardState {
        self.hot.read_state_filtered(filter)
    }
}

// ── Memory management: delegate to both hot and cold ──

impl EvictCapable for DualStorage {
    fn approximate_size(&self) -> usize {
        self.hot.approximate_size() + self.cold.approximate_size()
    }

    fn evict_before(&self, before: &str) -> Result<u64, String> {
        let hot_evicted = self.hot.evict_before(before)?;
        let cold_evicted = self.cold.evict_before(before)?;
        Ok(hot_evicted + cold_evicted)
    }

    fn evict_stale_intents(&self, older_than_secs: u64) -> Result<u64, String> {
        let hot_evicted = self.hot.evict_stale_intents(older_than_secs)?;
        let cold_evicted = self.cold.evict_stale_intents(older_than_secs)?;
        Ok(hot_evicted + cold_evicted)
    }
}

// ── Partition scan: merge hot (recent) + cold (flushed) data ──

impl ScanCapable for DualStorage {
    fn scan_partition(&self, partition: &str) -> Result<PartitionData, String> {
        let mut cold_data = self.cold.scan_partition(partition)?;
        let hot_state = self.hot.read_state();

        // Merge hot data into cold scan result (hot takes precedence).
        // Dedup by entity ID: hot facts override flushed facts.
        for fact in hot_state.facts {
            if !cold_data.facts.iter().any(|f| f.id == fact.id) {
                cold_data.facts.push(fact);
            }
        }
        for intent in hot_state.intents {
            if !cold_data.intents.iter().any(|i| i.id == intent.id) {
                cold_data.intents.push(intent);
            }
        }
        for hint in hot_state.hints {
            if !cold_data.hints.iter().any(|h| h.id == hint.id) {
                cold_data.hints.push(hint);
            }
        }
        cold_data.facts.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        cold_data.intents.sort_by(|a, b| a.id.0.cmp(&b.id.0));
        cold_data.hints.sort_by(|a, b| a.id.0.cmp(&b.id.0));

        Ok(cold_data)
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

// ── Flush: extract hot data, write to cold blob, advance cursor ──

impl FlushCapable for DualStorage {
    fn flush_since(&self, cursor: &FlushCursor) -> Result<FlushResult, String> {
        // Read only delta (entities submitted after cursor) from hot storage.
        let partition = &cursor.partition;
        let project_id = self.hot.project_id();
        let now_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_string();

        let (fact_lines, intent_lines, hint_lines) =
            self.hot.read_delta_since(&cursor.last_flushed_at);
        let records_flushed = (fact_lines.len() + intent_lines.len() + hint_lines.len()) as u64;

        if !fact_lines.is_empty() {
            let blob_key = format!("{project_id}/flush/facts/{partition}/{now_ts}.jsonl");
            self.cold
                .write_blob(&blob_key, fact_lines.join("\n").as_bytes())?;
        }
        if !intent_lines.is_empty() {
            let blob_key = format!("{project_id}/flush/intents/{partition}/{now_ts}.jsonl");
            self.cold
                .write_blob(&blob_key, intent_lines.join("\n").as_bytes())?;
        }
        if !hint_lines.is_empty() {
            let blob_key = format!("{project_id}/flush/hints/{partition}/{now_ts}.jsonl");
            self.cold
                .write_blob(&blob_key, hint_lines.join("\n").as_bytes())?;
        }

        // Advance cursor and persist to cold blob as a simple JSON file.
        let new_cursor = FlushCursor {
            last_flushed_at: now_ts,
            partition: partition.clone(),
        };
        let cursor_json =
            serde_json::to_string(&new_cursor).map_err(|e| format!("serialize cursor: {e}"))?;
        let cursor_key = format!("{project_id}/cursor.json");
        self.cold.write_blob(&cursor_key, cursor_json.as_bytes())?;

        Ok(FlushResult {
            records_flushed,
            new_cursor,
        })
    }
}

// ── Cypher query: delegate to hot (Petgraph implements CypherCapable) ──

impl CypherCapable for DualStorage {
    fn query_plan(&self, plan: &serde_json::Value) -> Result<serde_json::Value, String> {
        self.hot.query_plan(plan)
    }
}

impl HotStorage for DualStorage {
    fn read_delta_since(&self, cursor_ts: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
        self.hot.read_delta_since(cursor_ts)
    }
}
