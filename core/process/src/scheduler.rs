// nexus-process — Scheduler: OODA loop polling, Intent dispatch, heartbeat monitor.
//
// The scheduler drives the OODA cycle for any Blackboard implementation:
//   1. Poll `read_state()` for current board state
//   2. Run all registered detection tasks (GapDetection, ContradictionDetection, etc.)
//   3. Submit generated Intents and Facts
//   4. Monitor heartbeat TTL — release stale claims
//   5. Trigger periodic eviction when memory exceeds threshold
//      (always flush first, then evict — cold failure is non-fatal)
//
// Generic over `B: Blackboard + EvictCapable (+ optionally FlushCapable)`.
// Detection tasks implement `DetectionCapable` (or marker traits) from nexus-model.

use crate::error::ProcessError;
use nexus_model::{Blackboard, DetectionCapable, DetectionOutput, EvictCapable, TaskStates};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    pub tick_interval: Duration,
    pub eviction_threshold: usize,
    pub heartbeat_ttl: Duration,
    /// Maximum age for unclaimed, unconcluded intents before eviction.
    /// Default: 3600s (1 hour). Set to 0 to evict all stale intents immediately.
    pub stale_intent_ttl: Duration,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            tick_interval: Duration::from_millis(100),
            eviction_threshold: 1024 * 1024 * 10,
            heartbeat_ttl: Duration::from_secs(60),
            stale_intent_ttl: Duration::from_secs(3600),
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

    /// Run one OODA tick. Returns the number of new Facts submitted
    /// by detectors (detectors produce Facts, not Intents).
    ///
    /// When the backend also implements `FlushCapable`, eviction
    /// flushes cold storage first before evicting hot memory.
    /// Use `tick_with_flush()` for the flush+evict cycle.
    pub fn tick(&mut self) -> Result<usize, ProcessError> {
        self._tick_inner(|_bb| Ok(()))
    }

    /// Like `tick()` but runs the flush+evict cycle when memory
    /// exceeds threshold. Requires the backend to implement `FlushCapable`.
    pub fn tick_with_flush(&mut self) -> Result<usize, ProcessError>
    where
        B: nexus_model::FlushCapable,
    {
        let threshold = self.config.eviction_threshold;
        self._tick_inner(move |bb: &B| {
            let size = EvictCapable::approximate_size(bb);
            if size > threshold {
                crate::eviction::try_evict_flush(bb, threshold).map_err(ProcessError::Eviction)?;
            }
            Ok(())
        })
    }

    /// Inner tick logic shared between `tick()` and `tick_with_flush()`.
    /// The `evict_fn` parameter allows the caller to decide whether to
    /// use simple eviction or flush+evict.
    fn _tick_inner(
        &mut self,
        evict_fn: impl FnOnce(&B) -> Result<(), ProcessError>,
    ) -> Result<usize, ProcessError> {
        let state = Blackboard::read_state(&self.bb);

        let mut combined = DetectionOutput::default();
        for task in &mut self.tasks {
            let output = task.orient(&state);
            combined.intents.extend(output.intents);
            combined.facts.extend(output.facts);
        }

        let fact_count = combined.facts.len();
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

        evict_fn(&self.bb)?;

        // Evict stale unclaimed intents
        if self.config.stale_intent_ttl.as_secs() > 0 {
            let _ = self
                .bb
                .evict_stale_intents(self.config.stale_intent_ttl.as_secs());
        }

        Ok(fact_count)
    }

    pub fn run(&mut self, iterations: usize) -> Result<usize, ProcessError> {
        let mut total = 0;
        for _ in 0..iterations {
            total += self.tick()?;
        }
        Ok(total)
    }

    /// Collect serializable state from all registered detectors.
    pub fn collect_task_states(&self) -> TaskStates {
        let mut states = TaskStates::new();
        for task in &self.tasks {
            if let Some(state) = task.snapshot_state() {
                states.insert(task.name().to_string(), state);
            }
        }
        states
    }

    /// Restore detector state from a previously saved snapshot.
    pub fn restore_task_states(&mut self, states: &TaskStates) {
        for task in &mut self.tasks {
            if let Some(state) = states.get(task.name()) {
                task.restore_state(state.clone());
            }
        }
    }
}
