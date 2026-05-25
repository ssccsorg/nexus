// nexus-process — Scheduler: OODA loop polling, Intent dispatch, heartbeat monitor.
//
// The scheduler drives the OODA cycle for any Blackboard implementation:
//   1. Poll `read_state()` for current board state
//   2. Run all registered detection tasks (GapDetection, ContradictionDetection, etc.)
//   3. Submit generated Intents and Facts
//   4. Monitor heartbeat TTL — release stale claims
//   5. Trigger periodic eviction when memory exceeds threshold
//
// Generic over `B: Blackboard + EvictCapable`.
// Detection tasks implement `DetectionCapable` (or marker traits) from nexus-model.

use crate::error::ProcessError;
use nexus_model::{Blackboard, DetectionCapable, DetectionOutput, EvictCapable};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    pub tick_interval: Duration,
    pub eviction_threshold: usize,
    pub heartbeat_ttl: Duration,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            tick_interval: Duration::from_millis(100),
            eviction_threshold: 1024 * 1024 * 10,
            heartbeat_ttl: Duration::from_secs(60),
        }
    }
}

pub struct Scheduler<B: Blackboard + EvictCapable> {
    pub bb: B,
    config: SchedulerConfig,
    tasks: Vec<Box<dyn DetectionCapable>>,
}

impl<B: Blackboard + EvictCapable> Scheduler<B> {
    pub fn new(bb: B) -> Self {
        Self {
            bb,
            config: SchedulerConfig::default(),
            tasks: Vec::new(),
        }
    }

    /// Register a detection task. Any type implementing `DetectionCapable`
    /// (or its marker subtraits like `GapDetection`, `ContradictionDetection`,
    /// `StateChangeDetection`) can be registered.
    pub fn register(&mut self, task: Box<dyn DetectionCapable>) {
        self.tasks.push(task);
    }

    pub fn tick(&mut self) -> Result<usize, ProcessError> {
        let state = Blackboard::read_state(&self.bb);

        let mut combined = DetectionOutput::default();
        for task in &mut self.tasks {
            let output = task.orient(&state);
            combined.intents.extend(output.intents);
            combined.facts.extend(output.facts);
        }

        let intent_count = combined.intents.len();
        for intent in &combined.intents {
            let _ = self.bb.submit_intent(intent);
        }
        for fact in &combined.facts {
            let _ = self.bb.submit_fact(fact);
        }

        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        for intent in &state.intents {
            if let Some(worker) = &intent.worker
                && let Some(ref hb_str) = intent.last_heartbeat_at
                && let Ok(hb_secs) = hb_str.parse::<u64>()
            {
                let elapsed = now_secs.saturating_sub(hb_secs);
                if elapsed > self.config.heartbeat_ttl.as_secs() {
                    let _ = self.bb.release_intent(&intent.id.0, worker);
                }
            }
        }

        let size = EvictCapable::approximate_size(&self.bb);
        if size > self.config.eviction_threshold {
            crate::eviction::try_evict(&self.bb, self.config.eviction_threshold)?;
        }

        Ok(intent_count)
    }

    pub fn run(&mut self, iterations: usize) -> Result<usize, ProcessError> {
        let mut total = 0;
        for _ in 0..iterations {
            total += self.tick()?;
        }
        Ok(total)
    }
}
