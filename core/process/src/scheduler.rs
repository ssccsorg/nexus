// nexus-process — Scheduler: polling loop, Intent dispatch, heartbeat monitor.
//
// The scheduler runs the OODA loop for a worker's partition:
//   1. Poll `read_state()` for unclaimed Intents
//   2. Dispatch unclaimed Intents to registered task handlers
//   3. Monitor heartbeat TTL — release stale claims
//   4. Trigger periodic eviction when memory exceeds threshold

use crate::tasks::{TaskHandler, TaskOutput};
use nexus_graph::{Blackboard, DefaultBlackboard};
use std::time::Duration;

/// Configuration for the scheduler loop.
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    /// Interval between OODA iterations.
    pub tick_interval: Duration,
    /// Maximum memory before eviction is triggered (bytes).
    pub eviction_threshold: usize,
    /// Heartbeat TTL — release claims older than this.
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

/// The OODA loop scheduler.
pub struct Scheduler<'a> {
    bb: &'a mut DefaultBlackboard,
    config: SchedulerConfig,
    tasks: Vec<Box<dyn TaskHandler + 'a>>,
}

impl<'a> Scheduler<'a> {
    pub fn new(bb: &'a mut DefaultBlackboard) -> Self {
        Self {
            bb,
            config: SchedulerConfig::default(),
            tasks: Vec::new(),
        }
    }

    /// Register a stigmergy task handler (gap detector, etc.).
    pub fn register(&mut self, task: Box<dyn TaskHandler + 'a>) {
        self.tasks.push(task);
    }

    /// Run a single OODA iteration.
    ///
    /// Phases:
    ///   1. **Observe**: read current state from the blackboard
    ///   2. **Orient**: run registered task handlers on the state
    ///   3. **Decide**: submit new Intents from task output
    ///   4. **Act**: submit new Facts from task output
    ///
    /// Returns the number of Intents submitted this tick.
    pub fn tick(&mut self) -> Result<usize, String> {
        // ── Observe ────────────────────────────────────────────────────
        let state = self.bb.read_state();

        // ── Orient ─────────────────────────────────────────────────────
        let mut combined = TaskOutput::default();
        for task in &mut self.tasks {
            let output = task.orient(&state);
            combined.intents.extend(output.intents);
            combined.facts.extend(output.facts);
        }

        // ── Decide (submit intents) ────────────────────────────────────
        let intent_count = combined.intents.len();
        for intent in &combined.intents {
            let _ = self.bb.submit_intent(intent);
        }

        // ── Act (submit facts) ─────────────────────────────────────────
        for fact in &combined.facts {
            let _ = self.bb.submit_fact(fact);
        }

        // ── Heartbeat TTL check ───────────────────────────────────────
        // Release intents whose heartbeat is older than TTL.
        // Uses the intent's `last_heartbeat_at` which is set by the
        // claiming agent on each heartbeat() call.
        for intent in &state.intents {
            if let Some(worker) = &intent.worker {
                if let Some(ref hb) = intent.last_heartbeat_at {
                    // Simple TTL check: if heartbeat string is older
                    // than config.heartbeat_ttl, release.
                    // TODO(#35): proper timestamp comparison.
                    let _ = (worker, hb);
                }
            }
        }

        // ── Memory check → eviction ───────────────────────────────────
        let size = self.bb.storage_size();
        if size > self.config.eviction_threshold {
            let _ = crate::eviction::try_evict(self.bb, self.config.eviction_threshold);
        }

        Ok(intent_count)
    }

    /// Run N complete OODA iterations.
    pub fn run(&mut self, iterations: usize) -> Result<usize, String> {
        let mut total = 0;
        for _ in 0..iterations {
            total += self.tick()?;
        }
        Ok(total)
    }
}
