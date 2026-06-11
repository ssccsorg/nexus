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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_as_of() {
        let idx = TimeIndex::new();
        idx.record(100, "f001");
        idx.record(200, "f002");
        idx.record(300, "f003");

        let at_150 = idx.as_of(150);
        assert_eq!(at_150.len(), 1);
        assert_eq!(at_150[0].1, "f001");

        let at_300 = idx.as_of(300);
        assert_eq!(at_300.len(), 3);
    }

    #[test]
    fn test_since() {
        let idx = TimeIndex::new();
        idx.record(100, "f001");
        idx.record(200, "f002");
        idx.record(300, "f003");

        let after_150 = idx.since(150);
        assert_eq!(after_150.len(), 2);
        assert_eq!(after_150[0].1, "f002");

        let after_300 = idx.since(300);
        assert_eq!(after_300.len(), 0);
    }

    #[test]
    fn test_range() {
        let idx = TimeIndex::new();
        idx.record(100, "f001");
        idx.record(200, "f002");
        idx.record(300, "f003");
        idx.record(400, "f004");

        let mid = idx.range(150, 350);
        assert_eq!(mid.len(), 2);
        assert_eq!(mid[0].1, "f002");
        assert_eq!(mid[1].1, "f003");
    }

    #[test]
    fn test_empty() {
        let idx = TimeIndex::new();
        assert!(idx.is_empty());
        assert_eq!(idx.as_of(999).len(), 0);
        assert_eq!(idx.since(0).len(), 0);
    }

    #[test]
    fn test_monotonic_preserved() {
        let idx = TimeIndex::new();
        // Simulate sequential timestamps
        for i in 0..1000 {
            idx.record((i * 10) as u64, &format!("f{:04}", i));
        }
        assert_eq!(idx.len(), 1000);
        // as_of at midpoint
        let half = idx.as_of(5000);
        assert_eq!(half.len(), 501); // 0..=500 inclusive
    }
}
