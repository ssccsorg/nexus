// try_evict: the eviction cycle logic.
//
// The cycle skips eviction below the size threshold, then delegates to
// `evict_before` with a cutoff derived from `cutoff_secs`. A recording
// backend pins the delegation without depending on wall-clock timing.

use std::sync::Mutex;

use nex::eviction::try_evict;
use nex_fih::{BoardState, EvictCapable, StorageRead};

/// EvictCapable backend that records the cutoff passed to `evict_before`.
struct RecordEvict {
    size: usize,
    evicted: u64,
    last_cutoff: Mutex<Option<u64>>,
}

impl RecordEvict {
    fn new(size: usize, evicted: u64) -> Self {
        Self {
            size,
            evicted,
            last_cutoff: Mutex::new(None),
        }
    }
}

impl StorageRead for RecordEvict {
    fn project_id(&self) -> &str {
        "evict-test"
    }

    fn read_state(&self) -> BoardState {
        BoardState {
            facts: Vec::new(),
            intents: Vec::new(),
            hints: Vec::new(),
        }
    }
}

impl EvictCapable for RecordEvict {
    fn approximate_size(&self) -> usize {
        self.size
    }

    fn evict_before(&self, before: &str) -> Result<u64, String> {
        *self.last_cutoff.lock().unwrap() = before.parse().ok();
        Ok(self.evicted)
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[test]
fn try_evict_skips_below_threshold() {
    let mut backend = RecordEvict::new(10, 3);
    let evicted = try_evict(&mut backend, 100, 60).unwrap();
    assert_eq!(evicted, 0, "below threshold: nothing to evict");
    assert!(
        backend.last_cutoff.lock().unwrap().is_none(),
        "evict_before must not be called below threshold"
    );
}

#[test]
fn try_evict_evicts_at_threshold_boundary() {
    let mut backend = RecordEvict::new(100, 2);
    let evicted = try_evict(&mut backend, 100, 60).unwrap();
    assert_eq!(evicted, 2, "size == threshold is not below it");
    assert!(backend.last_cutoff.lock().unwrap().is_some());
}

#[test]
fn try_evict_delegates_with_cutoff() {
    let mut backend = RecordEvict::new(200, 5);
    let before = now_secs();
    let evicted = try_evict(&mut backend, 100, 60).unwrap();
    assert_eq!(evicted, 5);
    let after = now_secs();

    // cutoff = now - 60, computed inside try_evict between the two reads.
    let cutoff = backend
        .last_cutoff
        .lock()
        .unwrap()
        .expect("evict_before must be called above threshold");
    assert!(
        cutoff <= after.saturating_sub(60) && cutoff >= before.saturating_sub(61),
        "cutoff {cutoff} outside [{}, {}]",
        before.saturating_sub(61),
        after.saturating_sub(60)
    );
}
