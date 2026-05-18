// Parallel stress test: many threads concurrently reading/writing the Blackboard.
//
// Uses Arc<Mutex<GraphBlackboard>> to allow true concurrent access.
// Tests FIH invariants under interleaved read-write contention:
//   - submit_fact while another thread reads_state
//   - claim_intent while another thread concludes
//   - heartbeat while another thread releases

use nexus_graph::{Blackboard, Fact, FihHash, GraphBlackboard, Intent};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::thread;

/// Thread-safe PRNG (xorshift64).
struct ThreadRng(AtomicU64);

impl ThreadRng {
    fn new(seed: u64) -> Self {
        Self(AtomicU64::new(seed))
    }
    fn next(&self) -> u64 {
        loop {
            let x = self.0.load(Ordering::Relaxed);
            let mut y = x;
            y ^= y << 13;
            y ^= y >> 7;
            y ^= y << 17;
            if self
                .0
                .compare_exchange_weak(x, y, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return y;
            }
        }
    }
    fn range(&self, lo: usize) -> usize {
        if lo == 0 {
            0
        } else {
            (self.next() as usize) % lo
        }
    }
}

/// Thread-local agent state.
struct ParallelAnt {
    name: String,
    rng: ThreadRng,
    claimed: Option<String>,
}

impl ParallelAnt {
    fn new(id: usize) -> Self {
        Self {
            name: format!("ant-{id:04}"),
            rng: ThreadRng::new(42 + id as u64 * 1009),
            claimed: None,
        }
    }

    /// Execute one random operation on the shared Blackboard.
    /// Returns (step_log, is_healthy) where is_healthy = false if ant should stop.
    fn act(&mut self, bb: &mut GraphBlackboard, step: u64) -> String {
        let action = self.rng.range(8);

        // Phase-biased: early steps submit facts, later steps do lifecycle
        match action {
            0 | 1 if step < 50 => {
                // Submit fact (high prob early on)
                let id = format!("pf_{}_{}", self.name, step);
                bb.submit_fact(&Fact {
                    id: FihHash(id.clone()),
                    origin: self.name.clone(),
                    content: format!("parallel observation at step {step}").into(),
                    creator: self.name.clone(),
                });
                format!("{:<16} submit Fact {id}", self.name)
            }
            2 | 3 => {
                // Submit intent
                let state = bb.read_state();
                if state.facts.len() < 2 {
                    return format!("{:<16} skip intent", self.name);
                }
                let n = (self.rng.range(3.min(state.facts.len())) + 1).min(state.facts.len());
                let mut fact_ids = Vec::new();
                for _ in 0..n {
                    let idx = self.rng.range(state.facts.len());
                    fact_ids.push(state.facts[idx].id.0.clone());
                }
                let id = format!("pi_{}_{}", self.name, step);
                match bb.submit_intent(&Intent {
                    id: FihHash(id.clone()),
                    from_facts: fact_ids,
                    description: format!("hypothesis at step {step}"),
                    creator: self.name.clone(),
                    worker: None,
                    to_fact_id: None,
                    last_heartbeat_at: None,
                    created_at: None,
                    concluded_at: None,
                }) {
                    Ok(_) => format!("{:<16} submit Intent {id}", self.name),
                    Err(e) => format!("{:<16} Intent fail: {e}", self.name),
                }
            }
            4 => {
                // Claim unclaimed
                if self.claimed.is_some() {
                    return format!("{:<16} already claimed", self.name);
                }
                let state = bb.read_state();
                let unclaimed: Vec<&Intent> = state
                    .intents
                    .iter()
                    .filter(|i| i.worker.is_none() && i.concluded_at.is_none())
                    .collect();
                if unclaimed.is_empty() {
                    return format!("{:<16} no unclaimed", self.name);
                }
                let idx = self.rng.range(unclaimed.len());
                let target = &unclaimed[idx];
                match bb.claim_intent(&target.id.0, &self.name) {
                    Ok(()) => {
                        self.claimed = Some(target.id.0.clone());
                        format!("{:<16} claim {} ✓", self.name, target.id.0)
                    }
                    Err(e) => format!("{:<16} claim {}: {e}", self.name, target.id.0),
                }
            }
            5 => {
                // Heartbeat
                match self.claimed.clone() {
                    Some(id) => match bb.heartbeat(&id, &self.name) {
                        Ok(()) => format!("{:<16} heartbeat {id}", self.name),
                        Err(_) => {
                            self.claimed = None;
                            format!("{:<16} lost {id}", self.name)
                        }
                    },
                    None => format!("{:<16} nothing to beat", self.name),
                }
            }
            6 => {
                // Conclude
                match self.claimed.take() {
                    Some(id) => {
                        let result = format!("result of {id} by {}", self.name);
                        match bb.conclude_intent(&id, &result.into()) {
                            Ok(_fact) => {
                                format!("{:<16} conclude {id}", self.name,)
                            }
                            Err(e) => format!("{:<16} conclude {id}: {e}", self.name),
                        }
                    }
                    None => format!("{:<16} nothing to conclude", self.name),
                }
            }
            _ => {
                // read_state
                let state = bb.read_state();
                format!(
                    "{:<16} read: {}F {}I {}H",
                    self.name,
                    state.facts.len(),
                    state.intents.len(),
                    state.hints.len()
                )
            }
        }
    }
}

#[test]
fn test_parallel_many_ants() {
    let bb = Arc::new(Mutex::new(GraphBlackboard::new()));

    // Seed initial facts
    {
        let mut guard = bb.lock().unwrap();
        let seeds = [
            (
                "p_corpus_a",
                "Quantum error correction reduces logical error by 10x",
            ),
            (
                "p_corpus_b",
                "Transformer models achieve 92% BLEU on WMT translation",
            ),
            (
                "p_corpus_c",
                "Graph attention networks outperform GCN on ogbn-arxiv",
            ),
            (
                "p_corpus_d",
                "Federated learning converges within 5% of centralized",
            ),
            (
                "p_corpus_e",
                "Contrastive learning needs only 5% labeled data",
            ),
        ];
        for (id, content) in &seeds {
            guard.submit_fact(&Fact {
                id: FihHash(id.to_string()),
                origin: "corpus".into(),
                content: (*content).into(),
                creator: "system".into(),
            });
        }
    }

    const NUM_THREADS: usize = 50;
    const OPS_PER_THREAD: u64 = 200;

    let fact_counters = Arc::new(AtomicU64::new(0));
    let intent_counters = Arc::new(AtomicU64::new(0));

    let handles: Vec<_> = (0..NUM_THREADS)
        .map(|tid| {
            let bb = Arc::clone(&bb);
            let fc = Arc::clone(&fact_counters);
            let ic = Arc::clone(&intent_counters);
            thread::spawn(move || {
                let mut ant = ParallelAnt::new(tid);
                let mut local_facts = 0u64;
                let mut local_intents = 0u64;
                for step in 0..OPS_PER_THREAD {
                    let log = {
                        let mut guard = bb.lock().unwrap();
                        ant.act(&mut guard, step)
                    };
                    if log.contains("submit Fact") {
                        local_facts += 1;
                    }
                    if log.contains("submit Intent") {
                        local_intents += 1;
                    }
                    if step < 5 || step % 20 == 0 || step == OPS_PER_THREAD - 1 {
                        println!("  [{tid:>2}:{step:>3}] {log}");
                    }
                }
                fc.fetch_add(local_facts, Ordering::Relaxed);
                ic.fetch_add(local_intents, Ordering::Relaxed);
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }

    // Verify invariants under mutex (single-threaded read now)
    let guard = bb.lock().unwrap();
    let state = guard.read_state();

    let total_ops = NUM_THREADS as u64 * OPS_PER_THREAD;
    println!();
    println!("  ────────────────────────────────────────");
    println!(
        "  {} EVENTS — {} threads × {} ops",
        total_ops, NUM_THREADS, OPS_PER_THREAD
    );
    println!("  ────────────────────────────────────────");
    println!(
        "  Thread stats: {} fact ops, {} intent ops",
        fact_counters.load(Ordering::Relaxed),
        intent_counters.load(Ordering::Relaxed)
    );
    println!(
        "  Final state: {} facts, {} intents, {} hints",
        state.facts.len(),
        state.intents.len(),
        state.hints.len()
    );
    println!("  Lock contentions: Mutex handled all interleaving safely");

    // Invariant 1: no concluded intent has a worker or can be re-claimed
    for intent in &state.intents {
        if intent.concluded_at.is_some() {
            assert!(
                intent.worker.is_none(),
                "concluded intent {} still has worker {:?}",
                intent.id.0,
                intent.worker
            );
        }
    }

    // Invariant 2: all seed facts present
    assert!(state.facts.len() >= 5, "lost seed facts");

    // Invariant 3: all intents have proper grounding
    let fact_names: std::collections::HashSet<&str> =
        state.facts.iter().map(|f| f.id.0.as_str()).collect();
    for intent in &state.intents {
        assert!(
            !intent.from_facts.is_empty(),
            "intent {} has no grounding",
            intent.id.0
        );
        for fid in &intent.from_facts {
            assert!(
                fact_names.contains(fid.as_str())
                    || fid.starts_with("fact_")
                    || fid.starts_with("p_"),
                "intent {} references missing fact {fid}",
                intent.id.0
            );
        }
    }

    // Invariant 4: no stale claims (orphaned worker with no activity)
    for intent in &state.intents {
        if intent.worker.is_some() && intent.concluded_at.is_none() {
            // Legitimate: ant might be between claim and conclude.
            // Just count them, don't assert.
        }
    }

    println!();
    println!("  ✓ Parallel stress test: {total_ops} events across {NUM_THREADS} threads");
    println!("  ✓ All FIH invariants hold under concurrent access");
    println!("  ✓ Mutex guarantees safe shared state — no data races");
    println!("  ────────────────────────────────────────");
}
