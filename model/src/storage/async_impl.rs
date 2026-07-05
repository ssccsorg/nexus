// ── Async storage traits: async counterparts of sync storage traits.
//
// These traits mirror the sync versions but use `async fn` (AFIT, Rust 1.75+).
// The `async_fn_in_trait` lint is suppressed because AFIT is stable and
// these traits are not used as trait objects.

#![allow(async_fn_in_trait)]

use crate::error::BlackboardError;
use crate::fih::{BoardState, Fact, FihHash, Hint, Intent};
use crate::storage::{FlushCursor, FlushResult, PartitionData, StateFilter};
use std::ops::Range;

/// Async counterpart of [`super::read::StorageRead`].
pub trait AsyncStorageRead {
    fn project_id(&self) -> &str;
    async fn read_state(&self) -> BoardState;
}

/// Async counterpart of [`super::fact::FactCapable`].
pub trait AsyncFactCapable: AsyncStorageRead {
    async fn submit_fact(&self, fact: &Fact) -> Result<FihHash, BlackboardError>;
}

/// Async counterpart of [`super::hint::HintCapable`].
pub trait AsyncHintCapable: AsyncStorageRead {
    async fn submit_hint(&self, hint: &Hint) -> Result<(), BlackboardError>;
}

/// Async counterpart of [`super::intent::IntentCapable`].
pub trait AsyncIntentCapable: AsyncStorageRead {
    async fn submit_intent(&self, intent: &Intent) -> Result<FihHash, BlackboardError>;
    async fn claim_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError>;
    async fn heartbeat(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError>;
    async fn release_intent(&self, intent_id: &str, agent: &str) -> Result<(), BlackboardError>;
    async fn conclude_intent(
        &self,
        intent_id: &str,
        result: &str,
    ) -> Result<crate::fih::Fact, BlackboardError>;
}

/// Async counterpart of [`super::filter::FilterCapable`].
pub trait AsyncFilterCapable: AsyncStorageRead {
    async fn read_state_filtered(&self, filter: &StateFilter) -> BoardState;
}

/// Async counterpart of [`super::evict::EvictCapable`].
pub trait AsyncEvictCapable: AsyncStorageRead {
    async fn approximate_size(&self) -> usize;
    async fn evict_before(&self, before: &str) -> Result<u64, String>;
    async fn evict_stale_intents(&self, older_than_secs: u64) -> Result<u64, String>;
}

/// Async counterpart of [`super::scan::ScanCapable`].
pub trait AsyncScanCapable: AsyncStorageRead {
    async fn scan_partition(&self, partition: &str) -> Result<PartitionData, String>;
}

/// Async counterpart of [`super::time_range::TimeRangeCapable`].
pub trait AsyncTimeRangeCapable: AsyncStorageRead {
    async fn time_range(&self) -> Option<Range<String>>;
}

/// Async counterpart of [`super::flush::FlushCapable`].
pub trait AsyncFlushCapable: AsyncStorageRead {
    async fn flush_since(&self, cursor: &FlushCursor) -> Result<FlushResult, String>;
}

/// Async counterpart of [`super::governance::GovernanceCapable`].
pub trait AsyncGovernanceCapable: AsyncStorageRead {
    async fn register_schema(&self, schema_id: &str, schema: &[u8]) -> String;
    async fn admit_fact(&self, schema_id: &str, data: &[u8]) -> Result<(), BlackboardError>;
    async fn check_hints(&self, value: i64) -> Result<(), BlackboardError>;
    async fn evidence_tip(&self) -> Option<String>;
    async fn governance_enabled(&self) -> bool;
    async fn set_governance(&self, enabled: bool);
}
