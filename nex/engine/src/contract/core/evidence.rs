// ── EvidenceChain: append-only SHA-256 hash chain ──────────────────────
//
// Tamper-evident audit trail for all governance-admitted state transitions.
// Pattern from mind-mem's HashChainV2 + EvidenceChain (SHA3-512 → SHA-256
// for wasm compatibility).
//
// Every transition is recorded as an EvidenceEntry linking:
//   prev_hash ← SHA-256(action_hash + action_type + timestamp) → chain_hash
//
// Tampering any entry changes its chain_hash and invalidates all
// subsequent entries. verify(from_seq) detects this.
//
// In-memory only in v1. Persistence to be added in a future iteration.

use std::sync::Mutex;

/// Hex-encoded SHA-256 hash of the chain genesis.
const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// A single entry in the evidence chain.
#[derive(Debug, Clone)]
pub struct EvidenceEntry {
    /// Sequential entry number (0-based).
    pub seq: u64,
    /// Chain hash of the previous entry (or GENESIS_HASH for entry 0).
    pub prev_hash: String,
    /// SHA-256 hex of the action content (e.g., fact hash or intent hash).
    pub action_hash: String,
    /// Action type label (e.g., "fact", "intent", "conclude").
    pub action_type: String,
    /// Nanosecond timestamp (from SystemTime::now()).
    pub timestamp_ns: u64,
    /// SHA-256(prev_hash || action_hash || action_type || timestamp_ns).
    pub chain_hash: String,
}

// ── EvidenceChain ──────────────────────────────────────────────────────

/// Append-only chain of evidence entries linked by SHA-256 hashes.
///
/// Thread-safe via Mutex. All mutation is serialized so the in-memory
/// chain order is always consistent with the hash links.
pub struct EvidenceChain {
    entries: Mutex<Vec<EvidenceEntry>>,
}

impl EvidenceChain {
    /// Create a new empty evidence chain.
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
        }
    }

    /// Append a new entry to the chain.
    ///
    /// Computes the chain hash from the previous entry's hash, the action
    /// hash, the action type, and the timestamp. Returns the new chain hash.
    pub fn append(&self, action_hash: &str, action_type: &str, timestamp_ns: u64) -> String {
        use sha2::{Digest, Sha256};

        let mut entries = self.entries.lock().expect("EvidenceChain lock");
        let seq = entries.len() as u64;
        let prev_hash = entries
            .last()
            .map(|e| e.chain_hash.clone())
            .unwrap_or_else(|| GENESIS_HASH.to_string());

        // Compute chain_hash = SHA-256(prev_hash + action_hash + action_type + timestamp_ns)
        let mut h = Sha256::new();
        h.update(prev_hash.as_bytes());
        h.update(action_hash.as_bytes());
        h.update(action_type.as_bytes());
        h.update(timestamp_ns.to_le_bytes());
        let chain_hash = hex_encode(&h.finalize());

        entries.push(EvidenceEntry {
            seq,
            prev_hash,
            action_hash: action_hash.to_string(),
            action_type: action_type.to_string(),
            timestamp_ns,
            chain_hash: chain_hash.clone(),
        });

        chain_hash
    }

    /// Return the number of entries in the chain.
    pub fn len(&self) -> usize {
        self.entries.lock().expect("EvidenceChain lock").len()
    }

    /// Returns true if the chain is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Return the chain hash of the most recent entry, or `None` if empty.
    pub fn tip(&self) -> Option<String> {
        self.entries
            .lock()
            .expect("EvidenceChain lock")
            .last()
            .map(|e| e.chain_hash.clone())
    }

    /// Return the sha256 hash of the entire chain (hash of all chain hashes).
    /// This provides a single fingerprint for the full chain state.
    pub fn fingerprint(&self) -> Option<String> {
        use sha2::{Digest, Sha256};
        let entries = self.entries.lock().expect("EvidenceChain lock");
        if entries.is_empty() {
            return None;
        }
        let mut h = Sha256::new();
        for e in entries.iter() {
            h.update(e.chain_hash.as_bytes());
        }
        Some(hex_encode(&h.finalize()))
    }

    /// Verify chain integrity from `from_seq` to the tip.
    ///
    /// Recomputes each entry's chain_hash and checks it matches the stored
    /// value. Returns `true` if the chain is intact from `from_seq` onward.
    pub fn verify(&self, from_seq: u64) -> bool {
        let entries = self.entries.lock().expect("EvidenceChain lock");
        if entries.is_empty() {
            return true;
        }
        let start = from_seq as usize;
        if start >= entries.len() {
            return true;
        }

        use sha2::{Digest, Sha256};

        let mut prev = if start == 0 {
            GENESIS_HASH.to_string()
        } else {
            entries[start - 1].chain_hash.clone()
        };

        for e in entries.iter().skip(start) {
            let mut h = Sha256::new();
            h.update(prev.as_bytes());
            h.update(e.action_hash.as_bytes());
            h.update(e.action_type.as_bytes());
            h.update(e.timestamp_ns.to_le_bytes());
            let expected = hex_encode(&h.finalize());
            if e.chain_hash != expected {
                return false;
            }
            prev = e.chain_hash.clone();
        }
        true
    }

    /// Return all entries (for inspection/export).
    pub fn entries(&self) -> Vec<EvidenceEntry> {
        self.entries.lock().expect("EvidenceChain lock").clone()
    }

    /// Return a single entry by sequence number, if within range.
    pub fn get(&self, seq: u64) -> Option<EvidenceEntry> {
        let entries = self.entries.lock().expect("EvidenceChain lock");
        let idx = seq as usize;
        if idx < entries.len() {
            Some(entries[idx].clone())
        } else {
            None
        }
    }
}

impl Default for EvidenceChain {
    fn default() -> Self {
        Self::new()
    }
}

// ── Hex helper ─────────────────────────────────────────────────────────

fn hex_encode(bytes: &[u8]) -> String {
    crate::contract::core::util::hex_encode(bytes)
}
