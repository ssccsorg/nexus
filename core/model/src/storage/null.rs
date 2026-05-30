use std::ops::Range;
use std::time::{SystemTime, UNIX_EPOCH};

use super::aggregate::{ColdStorage, HotStorage};
use super::cypher::CypherCapable;
use super::evict::EvictCapable;
use super::fact::FactCapable;
use super::filter::{FilterCapable, StateFilter};
use super::flush::{FlushCapable, FlushCursor, FlushResult};
use super::hint::HintCapable;
use super::intent::IntentCapable;
use super::read::StorageRead;
use super::scan::{PartitionData, ScanCapable};
use super::time_range::TimeRangeCapable;
use crate::error::BlackboardError;
use crate::fih::{BoardState, Content, Fact, FihHash, Hint, Intent};

pub struct NullStorage;

impl NullStorage {
    fn default_project_id() -> &'static str {
        "default"
    }
}

impl StorageRead for NullStorage {
    fn project_id(&self) -> &str {
        Self::default_project_id()
    }

    fn read_state(&self) -> BoardState {
        BoardState {
            facts: Vec::new(),
            intents: Vec::new(),
            hints: Vec::new(),
        }
    }
}

impl FactCapable for NullStorage {
    fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError> {
        Ok(fact.id.clone())
    }
}

impl IntentCapable for NullStorage {
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        Ok(intent.id.clone())
    }
    fn claim_intent(&self, _id: &str, _agent: &str) -> Result<(), BlackboardError> {
        Ok(())
    }
    fn heartbeat(&self, _id: &str, _agent: &str) -> Result<(), BlackboardError> {
        Ok(())
    }
    fn release_intent(&self, _id: &str, _agent: &str) -> Result<(), BlackboardError> {
        Ok(())
    }
    fn conclude_intent(
        &self,
        _id: &str,
        _result: &str,
    ) -> Result<Fact, BlackboardError> {
        Ok(Fact {
            id: FihHash("null".into()),
            origin: String::new(),
            content: Content::Text("null".into()),
            creator: String::new(),
        })
    }
}

impl HintCapable for NullStorage {
    fn submit_hint(&self, _hint: &Hint) -> Result<(), BlackboardError> {
        Ok(())
    }
}

impl FilterCapable for NullStorage {
    fn read_state_filtered(&self, _filter: &StateFilter) -> BoardState {
        BoardState {
            facts: Vec::new(),
            intents: Vec::new(),
            hints: Vec::new(),
        }
    }
}

impl ScanCapable for NullStorage {
    fn scan_partition(&self, _partition: &str) -> Result<PartitionData, String> {
        Ok(PartitionData {
            partition: _partition.to_string(),
            facts: Vec::new(),
            intents: Vec::new(),
            hints: Vec::new(),
        })
    }
}

impl EvictCapable for NullStorage {
    fn approximate_size(&self) -> usize {
        0
    }
    fn evict_before(&self, _before: &str) -> Result<u64, String> {
        Ok(0)
    }
}

impl FlushCapable for NullStorage {
    fn flush_since(&self, cursor: &FlushCursor) -> Result<FlushResult, String> {
        let now_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string();
        Ok(FlushResult {
            records_flushed: 0,
            new_cursor: FlushCursor {
                last_flushed_at: now_ts,
                partition: cursor.partition.clone(),
            },
        })
    }
}

impl CypherCapable for NullStorage {}

impl HotStorage for NullStorage {
    fn read_delta_since(&self, _cursor_ts: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
        (Vec::new(), Vec::new(), Vec::new())
    }
}

impl ColdStorage for NullStorage {
    fn write_blob(&self, _key: &str, _data: &[u8]) -> Result<(), String> {
        Ok(())
    }
}

impl TimeRangeCapable for NullStorage {
    fn time_range(&self) -> Option<Range<String>> {
        None
    }
}
