// ── Test helpers: shared across all storage/sim test files ─────────────

use std::sync::{Arc, Mutex};

use nex_fih::{Content, CoordId, Fact, Intent};

// ── FakeClock ──────────────────────────────────────────────────────────

/// Cloneable fake clock: the shared handle lets a test advance the clock
/// after it has been moved into a `FihStorage` via `with_clock`.
#[derive(Clone)]
pub struct FakeClock {
    now: Arc<Mutex<u64>>,
    step_ns: u64,
}

#[allow(dead_code)]
impl FakeClock {
    pub fn new(start_ns: u64) -> Self {
        Self {
            now: Arc::new(Mutex::new(start_ns)),
            step_ns: 1_000_000,
        }
    }

    #[allow(dead_code)]
    pub fn with_step(start_ns: u64, step_ns: u64) -> Self {
        Self {
            now: Arc::new(Mutex::new(start_ns)),
            step_ns,
        }
    }

    /// Advance the clock by whole seconds, for second-granularity
    /// timestamps such as hint and intent submission times.
    #[allow(dead_code)]
    pub fn advance_secs(&self, secs: u64) {
        let mut now = self.now.lock().unwrap();
        *now += secs * 1_000_000_000;
    }
}

impl nex_core::Now for FakeClock {
    fn now_nanos(&self) -> u64 {
        let mut now = self.now.lock().unwrap();
        let ts = *now;
        *now += self.step_ns;
        ts
    }

    fn now_secs(&self) -> u64 {
        let now = self.now.lock().unwrap();
        *now / 1_000_000_000
    }
}

// ── Fact / Intent helpers ──────────────────────────────────────────────

#[allow(dead_code)]
pub fn fact(id: &str) -> Fact {
    Fact::with_id(
        CoordId::from_string(id),
        "t".into(),
        Content {
            mime_type: "text/plain".into(),
            data: id.as_bytes().to_vec(),
        },
        "t".into(),
    )
}

#[allow(dead_code)]
pub fn intent(id: &str, from: Vec<&str>) -> Intent {
    Intent {
        id: CoordId::from_string(id),
        from_facts: from.into_iter().map(|s| CoordId::from_string(s)).collect(),
        description: format!("intent {}", id),
        creator: "t".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    }
}
