use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::ops::Range;

use crate::error::BlackboardError;
use crate::fih::{BoardState, Content, CoordId, Fact, Hint, Intent};
use crate::storage::aggregate::ColdStorage;
use crate::storage::evict::EvictCapable;
use crate::storage::fact::FactCapable;
use crate::storage::filter::{FilterCapable, StateFilter};
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
    fn submit_fact(&self, fact: &Fact) -> Result<CoordId, BlackboardError> {
        Ok(fact.id)
    }
}

impl IntentCapable for NullStorage {
    fn submit_intent(&self, intent: &Intent) -> Result<CoordId, BlackboardError> {
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
        Ok(Fact::with_id(
            CoordId::from_label("null"),
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
