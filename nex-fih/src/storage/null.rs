use std::ops::Range;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::BlackboardError;
use crate::fih::{BoardState, Content, Fact, FihHash, Hint, Intent};
use crate::storage::aggregate::{ColdStorage, DeltaSet, HotStorage};
use crate::storage::evict::EvictCapable;
use crate::storage::fact::FactCapable;
use crate::storage::filter::{FilterCapable, StateFilter};
use crate::storage::flush::{FlushCapable, FlushCursor, FlushResult};
use crate::storage::hint::HintCapable;
use crate::storage::intent::IntentCapable;
use crate::storage::read::StorageRead;
use crate::storage::scan::{PartitionData, ScanCapable};
use crate::storage::time_range::TimeRangeCapable;

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
        Ok(fact.id)
    }
}

impl IntentCapable for NullStorage {
    fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError> {
        Ok(intent.id)
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
    fn conclude_intent(&self, _id: &str, _result: &str) -> Result<Fact, BlackboardError> {
        Ok(Fact::new(
            FihHash::from_hex("null"),
            String::new(),
            Content::from("null"),
            String::new(),
        ))
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
            .as_secs();
        Ok(FlushResult {
            records_flushed: 0,
            new_cursor: FlushCursor {
                last_flushed_at: now_ts,
                partition: cursor.partition.clone(),
            },
        })
    }
}

impl HotStorage for NullStorage {
    fn read_delta_since(&self, _cursor_ts: &str) -> DeltaSet {
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
