// ── Test helpers: shared across all storage/sim test files ─────────────

use std::sync::Mutex;

use nexus_model::{Content, Fact, FihHash, Intent};

// ── FakeClock ──────────────────────────────────────────────────────────

pub struct FakeClock {
    now: Mutex<u64>,
    step_ns: u64,
}

impl FakeClock {
    #[allow(dead_code)]
    pub fn new(start_ns: u64) -> Self {
        Self {
            now: Mutex::new(start_ns),
            step_ns: 1_000_000,
        }
    }

    pub fn with_step(start_ns: u64, step_ns: u64) -> Self {
        Self {
            now: Mutex::new(start_ns),
            step_ns,
        }
    }
}

impl nexus_model::Now for FakeClock {
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

pub fn fact(id: &str) -> Fact {
    Fact {
        id: FihHash(id.into()),
        origin: "t".into(),
        content: Content {
            mime_type: "text/plain".into(),
            data: id.as_bytes().to_vec(),
        },
        creator: "t".into(),
    }
}

pub fn intent(id: &str, from: Vec<&str>) -> Intent {
    Intent {
        id: FihHash(id.into()),
        from_facts: from.into_iter().map(|s| s.to_string()).collect(),
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
