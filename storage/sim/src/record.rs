// ── FIH record types ────────────────────────────────────────────────────
//
// Canonical on-disk record formats for Fact, Intent, and Hint.
// Serialized with bincode. Each record is stored as a separate file.
//
// Key paths:
//   facts/f_{hash}.fact         → FactRecord
//   intents/i_{hash}.intent     → IntentRecord
//   hints/h_{hash}.hint         → HintRecord
//
// blob/{blob_hash}.bin          → raw content bytes
// blob/{blob_hash}.bin.meta     → ContentMeta (mime_type)

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
    pub id: String,            // FihHash.0
    pub blob_hash: BlobHash,   // → blob/{hash}.bin
    pub origin: String,
    pub creator: String,
    pub submitted_at: String,  // nanosecond timestamp string
}

/// Intent lifecycle state, enforced at the type level.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum IntentStatus {
    /// Intent has been submitted but not yet claimed by any worker.
    Submitted,
    /// A worker has claimed this intent and is actively working on it.
    Claimed {
        worker: String,
        last_heartbeat_at: u64,
    },
    /// A worker has concluded this intent, producing a result Fact.
    Concluded {
        to_fact: String,      // FihHash of the conclusion Fact
        concluded_at: u64,
        worker: String,       // permanent record of who concluded
    },
}

/// On-disk Intent record. State machine: Submitted → Claimed → Concluded.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IntentRecord {
    pub id: String,
    pub from_facts: Vec<String>,
    pub description_hash: BlobHash,  // → blob/{hash}.bin
    pub creator: String,
    pub status: IntentStatus,
    pub created_at: u64,
}

/// On-disk Hint record. Ephemeral — may be garbage-collected by TTL.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HintRecord {
    pub id: String,
    pub content: String,
    pub creator: String,
    pub submitted_at: u64,
    pub ttl_secs: Option<u64>,  // None = permanent
}

impl FactRecord {
    pub fn key(&self) -> String {
        format!("facts/f_{}.fact", self.id)
    }

    pub fn blob_key(&self) -> String {
        format!("blob/{}.bin", self.blob_hash)
    }

    pub fn blob_meta_key(&self) -> String {
        format!("blob/{}.bin.meta", self.blob_hash)
    }
}

impl IntentRecord {
    pub fn key(&self) -> String {
        format!("intents/i_{}.intent", self.id)
    }
}

impl HintRecord {
    pub fn key(&self) -> String {
        format!("hints/h_{}.hint", self.id)
    }

    pub fn is_expired(&self, now_secs: u64) -> bool {
        self.ttl_secs
            .map(|ttl| self.submitted_at + ttl < now_secs)
            .unwrap_or(false)
    }
}

// ── Conversions between nexus_model types and sim record types ──────────

use nexus_model::Content;

impl FactRecord {
    /// Build a FactRecord from a nexus_model::Fact, given a pre-computed
    /// blob_hash and submitted_at timestamp.
    pub fn from_model(fact: &nexus_model::Fact, blob_hash: BlobHash, submitted_at: &str) -> Self {
        Self {
            id: fact.id.0.clone(),
            blob_hash,
            origin: fact.origin.clone(),
            creator: fact.creator.clone(),
            submitted_at: submitted_at.to_string(),
        }
    }
}


