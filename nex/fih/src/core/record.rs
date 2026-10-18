// ── FIH record types ────────────────────────────────────────────────────
//
// Canonical on-disk record formats for Fact, Intent, and Hint.
// Serialized with postcard. Each record is stored as a separate file.
//
// Key paths:
//   facts/f_{hash}.fact         → FactRecord
//   intents/i_{hash}.intent     → IntentRecord
//   hints/h_{hash}.hint         → HintRecord
//
// blob/{blob_hash}.bin          → raw content bytes
// blob/{blob_hash}.bin.meta     → ContentMeta (mime_type)

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// Content-addressable blob hash (SHA-256 of content bytes).
pub type BlobHash = String;

/// Metadata for a stored blob.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContentMeta {
    pub mime_type: String,
    pub size: u64,
}

/// On-disk Fact record. Append-only, immutable after write.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactRecord {
    pub id: String,          // FihHash.0
    pub blob_hash: BlobHash, // → blob/{hash}.bin
    pub origin: String,
    pub creator: String,
    pub submitted_at: u64, // nanosecond timestamp
}

/// Intent lifecycle state, enforced at the type level.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum IntentStatus {
    /// Intent has been submitted but not yet claimed by any worker.
    Submitted,
    /// A worker has claimed this intent and is actively working on it.
    Claimed {
        worker: String,
        /// The last heartbeat, in nanoseconds since the UNIX epoch.
        last_heartbeat_at: u64,
    },
    /// A worker has concluded this intent, producing a result Fact.
    Concluded {
        to_fact: String, // FihHash of the conclusion Fact
        /// When the conclusion happened, in nanoseconds since the UNIX epoch.
        concluded_at: u64,
        worker: String, // permanent record of who concluded
    },
}

/// The unit of the timestamps in a persisted intent or hint record.
///
/// A record written before the nanosecond convention has no field and decodes through
/// the legacy shape as `Seconds`. A record written now carries `Nanos`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecordTimeUnit {
    Seconds,
    Nanos,
}

/// On-disk Intent record. State machine: Submitted → Claimed → Concluded.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntentRecord {
    pub id: String,
    pub from_facts: Vec<String>,
    pub description_hash: BlobHash, // → blob/{hash}.bin
    pub creator: String,
    pub status: IntentStatus,
    /// When the intent was created, in the unit `time_unit` states.
    pub created_at: u64,
    /// The unit of `created_at` and the status timestamps.
    pub time_unit: RecordTimeUnit,
}

/// On-disk Hint record. Ephemeral — may be garbage-collected by TTL.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HintRecord {
    pub id: String,
    pub content: String,
    pub creator: String,
    /// When the hint was submitted, in the unit `time_unit` states.
    pub submitted_at: u64,
    pub ttl_secs: Option<u64>, // None = permanent
    /// The unit of `submitted_at`.
    pub time_unit: RecordTimeUnit,
}

/// The shape of an intent record written before `time_unit` existed. Its timestamps are
/// seconds.
#[derive(Debug, Deserialize)]
pub(crate) struct IntentRecordV1 {
    id: String,
    from_facts: Vec<String>,
    description_hash: BlobHash,
    creator: String,
    status: IntentStatus,
    created_at: u64,
}

/// The shape of a hint record written before `time_unit` existed. Its timestamp is
/// seconds.
#[derive(Debug, Deserialize)]
pub(crate) struct HintRecordV1 {
    id: String,
    content: String,
    creator: String,
    submitted_at: u64,
    ttl_secs: Option<u64>,
}

fn lift_legacy_seconds(seconds: u64) -> u64 {
    seconds.saturating_mul(1_000_000_000)
}

fn lift_status(status: IntentStatus) -> IntentStatus {
    match status {
        IntentStatus::Claimed {
            worker,
            last_heartbeat_at,
        } => IntentStatus::Claimed {
            worker,
            last_heartbeat_at: lift_legacy_seconds(last_heartbeat_at),
        },
        IntentStatus::Concluded {
            to_fact,
            concluded_at,
            worker,
        } => IntentStatus::Concluded {
            to_fact,
            concluded_at: lift_legacy_seconds(concluded_at),
            worker,
        },
        other => other,
    }
}

impl IntentRecordV1 {
    pub(crate) fn into_current(self) -> IntentRecord {
        IntentRecord {
            id: self.id,
            from_facts: self.from_facts,
            description_hash: self.description_hash,
            creator: self.creator,
            status: lift_status(self.status),
            created_at: lift_legacy_seconds(self.created_at),
            time_unit: RecordTimeUnit::Nanos,
        }
    }
}

impl HintRecordV1 {
    pub(crate) fn into_current(self) -> HintRecord {
        HintRecord {
            id: self.id,
            content: self.content,
            creator: self.creator,
            submitted_at: lift_legacy_seconds(self.submitted_at),
            ttl_secs: self.ttl_secs,
            time_unit: RecordTimeUnit::Nanos,
        }
    }
}

/// Decode an intent record, lifting one written before the unit field existed.
pub(crate) fn decode_intent_record(bytes: &[u8]) -> Result<IntentRecord, postcard::Error> {
    match postcard::from_bytes::<IntentRecord>(bytes) {
        Ok(mut record) => {
            if record.time_unit == RecordTimeUnit::Seconds {
                record.created_at = lift_legacy_seconds(record.created_at);
                record.status = lift_status(record.status);
                record.time_unit = RecordTimeUnit::Nanos;
            }
            Ok(record)
        }
        Err(current_error) => match postcard::from_bytes::<IntentRecordV1>(bytes) {
            Ok(legacy) => Ok(legacy.into_current()),
            Err(_) => Err(current_error),
        },
    }
}

/// Decode a hint record, lifting one written before the unit field existed.
pub(crate) fn decode_hint_record(bytes: &[u8]) -> Result<HintRecord, postcard::Error> {
    match postcard::from_bytes::<HintRecord>(bytes) {
        Ok(mut record) => {
            if record.time_unit == RecordTimeUnit::Seconds {
                record.submitted_at = lift_legacy_seconds(record.submitted_at);
                record.time_unit = RecordTimeUnit::Nanos;
            }
            Ok(record)
        }
        Err(current_error) => match postcard::from_bytes::<HintRecordV1>(bytes) {
            Ok(legacy) => Ok(legacy.into_current()),
            Err(_) => Err(current_error),
        },
    }
}

impl FactRecord {
    /// The key a fact with this identifier lives at.
    ///
    /// The record's address is computed from what it holds, so the key follows from the
    /// identifier alone: a reader that has one and not the other still finds the record.
    pub fn fact_key(id: &str) -> String {
        format!("facts/f_{id}.fact")
    }

    pub fn key(&self) -> String {
        Self::fact_key(&self.id)
    }

    pub fn blob_key(&self) -> String {
        format!("blob/{}.bin", self.blob_hash)
    }

    pub fn blob_meta_key(&self) -> String {
        format!("blob/{}.bin.meta", self.blob_hash)
    }
}

impl IntentRecord {
    /// The key an intent with this identifier lives at.
    ///
    /// The address is the identifier the caller supplied, so the key follows from it alone.
    pub fn intent_key(id: &str) -> String {
        format!("intents/i_{id}.intent")
    }

    pub fn key(&self) -> String {
        Self::intent_key(&self.id)
    }
}

impl HintRecord {
    /// The key a hint with this identifier lives at.
    pub fn hint_key(id: &str) -> String {
        format!("hints/h_{id}.hint")
    }

    pub fn key(&self) -> String {
        Self::hint_key(&self.id)
    }

    /// Whether the hint's TTL has elapsed at `now_nanos`.
    ///
    /// The record timestamp is nanoseconds and the TTL is seconds, so the TTL is
    /// converted once here rather than being added to the timestamp.
    pub fn is_expired(&self, now_nanos: u64) -> bool {
        self.ttl_secs
            .map(|ttl_secs| {
                self.submitted_at
                    .saturating_add(ttl_secs.saturating_mul(1_000_000_000))
                    < now_nanos
            })
            .unwrap_or(false)
    }
}

// ── Conversions between nex_fih types and storage record types ──────────

impl FactRecord {
    /// Build a FactRecord from a nex_fih::Fact, given a pre-computed
    /// blob_hash and submitted_at timestamp.
    pub fn from_model(fact: &crate::Fact, blob_hash: BlobHash, submitted_at: u64) -> Self {
        Self {
            id: fact.id.to_string(),
            blob_hash,
            origin: fact.origin.clone(),
            creator: fact.creator.clone(),
            submitted_at,
        }
    }
}
