// ── FakeClock: deterministic clock for testing ─────────────────────────

use std::sync::Mutex;

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
    fn now_nanos(&self) -> String {
        let mut now = self.now.lock().unwrap();
        let ts = *now;
        *now += self.step_ns;
        ts.to_string()
    }

    fn now_secs(&self) -> u64 {
        let now = self.now.lock().unwrap();
        *now / 1_000_000_000
    }
}
