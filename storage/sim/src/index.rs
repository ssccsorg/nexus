// ── TimeIndex: append-only temporal index with binary-search seek ──────
//
// FIH submitted_at is monotonically increasing within a single storage
// instance, so push order equals time order. Binary search via
// partition_point() provides O(log N) seek for time-travel queries
// (as_of, since, delta flush cursor).
//
// Memory: contiguous Vec — cache-friendly, no per-node allocation.

use std::sync::RwLock;

/// Append-only time index. Thread-safe via RwLock.
pub struct TimeIndex {
    /// (timestamp_nanos, fact_id) pairs in insertion order.
    /// submitted_at is monotonically increasing, so push order == time order.
    entries: RwLock<Vec<(u64, String)>>,
}

impl TimeIndex {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(Vec::new()),
        }
    }

    /// Record a new timestamp-fact pair. O(1) amortized.
    pub fn record(&self, ts: u64, id: &str) {
        self.entries.write().unwrap().push((ts, id.to_string()));
    }

    /// Return all entries with timestamp <= ts (time-travel "as of").
    /// O(log N) seek + O(K) output.
    pub fn as_of(&self, ts: u64) -> Vec<(u64, String)> {
        let entries = self.entries.read().unwrap();
        let end = entries.partition_point(|(t, _)| *t <= ts);
        entries[..end].to_vec()
    }

    /// Return all entries with timestamp > ts (delta since cursor).
    /// O(log N) seek + O(K) output.
    pub fn since(&self, ts: u64) -> Vec<(u64, String)> {
        let entries = self.entries.read().unwrap();
        let start = entries.partition_point(|(t, _)| *t <= ts);
        entries[start..].to_vec()
    }

    /// Return all entries with start_ts <= timestamp < end_ts.
    /// O(log N) seek + O(K) output.
    pub fn range(&self, start_ts: u64, end_ts: u64) -> Vec<(u64, String)> {
        let entries = self.entries.read().unwrap();
        let start = entries.partition_point(|(t, _)| *t < start_ts);
        let end = entries.partition_point(|(t, _)| *t < end_ts);
        entries[start..end].to_vec()
    }

    /// Total number of recorded entries. O(1).
    pub fn len(&self) -> usize {
        self.entries.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.read().unwrap().is_empty()
    }

    /// Earliest timestamp in the index, or None if empty. O(1).
    pub fn first_ts(&self) -> Option<u64> {
        self.entries
            .read()
            .ok()
            .and_then(|e| e.first().map(|(ts, _)| *ts))
    }

    /// Latest timestamp in the index, or None if empty. O(1).
    pub fn last_ts(&self) -> Option<u64> {
        self.entries
            .read()
            .ok()
            .and_then(|e| e.last().map(|(ts, _)| *ts))
    }

    /// Drain all entries (for testing). O(1).
    pub fn clear(&self) {
        self.entries.write().unwrap().clear();
    }
}

impl Default for TimeIndex {
    fn default() -> Self {
        Self::new()
    }
}
