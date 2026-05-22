// nexus-process — Scheduler: OODA loop polling, Intent dispatch, heartbeat monitor.
//
// The scheduler drives the OODA cycle for any Blackboard implementation:
//   1. Poll `read_state()` for unclaimed Intents
//   2. Dispatch to registered stigmergy task handlers
//   3. Monitor heartbeat TTL — release stale claims
//   4. Trigger periodic eviction when memory exceeds threshold
//
// Generic over `B: Blackboard + EvictCapable`. The caller may use
// `DefaultBlackboard`, `Arc<Mutex<DefaultBlackboard>>`, or any future
// implementation — only the trait protocol matters.

use crate::tasks::{TaskHandler, TaskOutput};
use nexus_model::{Blackboard, EvictCapable};
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

/// The OODA loop scheduler. Generic over any `Blackboard + EvictCapable`.
///
/// The blackboard is consumed (owned), not borrowed. For shared access,
/// pass `Arc<Mutex<B>>` which implements `Blackboard` via the blanket impl
/// on `&mut T`.
pub struct Scheduler<B: Blackboard + EvictCapable> {
    pub bb: B,
    config: SchedulerConfig,
    tasks: Vec<Box<dyn TaskHandler>>,
}

impl<B: Blackboard + EvictCapable> Scheduler<B> {
    pub fn new(bb: B) -> Self {
        Self {
            bb,
            config: SchedulerConfig::default(),
            tasks: Vec::new(),
        }
    }

    /// Register a stigmergy task handler (gap detector, etc.).
    pub fn register(&mut self, task: Box<dyn TaskHandler>) {
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
        let state = Blackboard::read_state(&self.bb);

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
            // TODO(#35): proper timestamp comparison and release
            if let Some(worker) = &intent.worker
                && intent.last_heartbeat_at.is_some()
            {
                let _ = worker;
            }
        }

        // ── Memory check → eviction ───────────────────────────────────
        let size = EvictCapable::approximate_size(&self.bb);
        if size > self.config.eviction_threshold {
            let _ = crate::eviction::try_evict(&self.bb, self.config.eviction_threshold);
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
