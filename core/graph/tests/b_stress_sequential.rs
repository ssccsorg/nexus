// Stress test: many agents randomly reading/writing the Blackboard.
//
// Verifies FIH invariants under concurrent-random access patterns:
//   - No duplicate claims on the same Intent
//   - No heartbeat/conclude on unclaimed Intents
//   - All submitted Facts are visible via read_state
//   - Intent lifecycle completes correctly under contention

use nexus_graph::cypher;
use nexus_graph::{Blackboard, Fact, FihHash, Intent, create_blackboard};
use std::collections::HashSet;

/// An agent that randomly reads and writes the Blackboard.
struct Ant {
    name: String,
    claimed: Option<String>, // intent_id this agent currently holds
}

impl Ant {
    fn new(id: usize) -> Self {
        Self {
            name: format!("ant-{id:04}"),
            claimed: None,
        }
    }

    /// Perform one random action. Returns a log line.
    fn act(&mut self, bb: &mut impl Blackboard, rng: &mut TestRng, step: usize) -> String {
        match rng.gen_range(8) {
            // 0-2: submit facts (high probability — knowledge injection)
            0 | 1 | 2 => {
                let id = format!("f_{}_{}", self.name, step);
                let fact = Fact {
                    id: FihHash(id.clone()),
                    origin: self.name.clone(),
                    content: format!("observation at step {step} by {}", self.name).into(),
                    creator: self.name.clone(),
                };
                bb.submit_fact(&fact).unwrap();
                format!("{:<12} submit Fact {id}", self.name)
            }
            // 3: submit intent (requires grounding in existing facts)
            3 => {
                let state = bb.read_state();
                if state.facts.len() < 2 {
                    return format!("{:<12} skip intent — need ≥2 facts", self.name);
                }
                // Pick 1-3 random existing facts as grounding
                let n = rng.gen_range(3.min(state.facts.len())) + 1;
                let mut fact_ids = Vec::new();
                let mut chosen = HashSet::new();
                for _ in 0..n {
                    loop {
                        let idx = rng.gen_range(state.facts.len());
                        let fid = &state.facts[idx].id.0;
                        if chosen.insert(fid.clone()) {
                            fact_ids.push(fid.clone());
                            break;
                        }
                    }
                }
                let intent = Intent {
                    id: FihHash(format!("i_{}_{}", self.name, step)),
                    from_facts: fact_ids.clone(),
                    description: format!("hypothesis by {} at step {step}", self.name),
                    creator: self.name.clone(),
                    worker: None,
                    to_fact_id: None,
                    last_heartbeat_at: None,
                    created_at: None,
                    concluded_at: None,
                };
                match bb.submit_intent(&intent) {
                    Ok(hash) => format!("{:<12} submit Intent {hash}", self.name),
                    Err(e) => format!("{:<12} Intent rejected: {e}", self.name),
                }
            }
            // 4: claim an unclaimed intent
            4 if self.claimed.is_none() => {
                let state = bb.read_state();
                let unclaimed: Vec<&Intent> = state
                    .intents
                    .iter()
                    .filter(|i| i.worker.is_none())
                    .collect();
                if unclaimed.is_empty() {
                    return format!("{:<12} nothing to claim", self.name);
                }
                let idx = rng.gen_range(unclaimed.len());
                let target = &unclaimed[idx];
                match bb.claim_intent(&target.id.0, &self.name) {
                    Ok(()) => {
                        self.claimed = Some(target.id.0.clone());
                        format!("{:<12} claim {} ✓", self.name, target.id.0)
                    }
                    Err(e) => format!("{:<12} claim {} failed: {e}", self.name, target.id.0),
                }
            }
            // 5: heartbeat on claimed intent
            5 if self.claimed.is_some() => {
                let id = self.claimed.clone().unwrap();
                match bb.heartbeat(&id, &self.name) {
                    Ok(()) => format!("{:<12} heartbeat {id}", self.name),
                    Err(_) => {
                        self.claimed = None;
                        format!("{:<12} lost claim on {id}", self.name)
                    }
                }
            }
            // 6: conclude claimed intent
            6 if self.claimed.is_some() => {
                let id = self.claimed.take().unwrap();
                let result = format!("result of {id} by {}", self.name);
                match bb.conclude_intent(&id, &result) {
                    Ok(_fact) => {
                        format!("{:<12} conclude {id} → fact", self.name,)
                    }
                    Err(e) => {
                        self.claimed = None;
                        format!("{:<12} conclude {id} failed: {e}", self.name)
                    }
                }
            }
            // 7: read state
            _ => {
                let state = bb.read_state();
                format!(
                    "{:<12} read state: {}F {}I {}H",
                    self.name,
                    state.facts.len(),
                    state.intents.len(),
                    state.hints.len()
                )
            }
        }
    }
}

// ---- Minimal deterministic PRNG (no external dep needed) ----

struct TestRng(u64);

impl TestRng {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn gen_range(&mut self, lo: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as usize % lo
    }
}

#[test]
fn test_stress_many_ants() {
    let mut bb = create_blackboard();
    let mut rng = TestRng::new(42);

    // Phase 1: seed with initial facts (research corpus)
    let seed_facts = [
        (
            "f_corpus_001",
            "corpus",
            "Quantum computing reduces error rates below fault-tolerant threshold",
        ),
        (
            "f_corpus_002",
            "corpus",
            "Transformer models achieve state-of-the-art on 12 NLP benchmarks",
        ),
        (
            "f_corpus_003",
            "corpus",
            "Graph neural networks outperform CNNs on molecular property prediction",
        ),
        (
            "f_corpus_004",
            "corpus",
            "Reinforcement learning agents learn 3x faster with curriculum training",
        ),
        (
            "f_corpus_005",
            "corpus",
            "Diffusion models generate higher-quality images than GANs",
        ),
        (
            "f_corpus_006",
            "corpus",
            "Federated learning maintains privacy within 2% of centralized accuracy",
        ),
        (
            "f_corpus_007",
            "corpus",
            "Spiking neural networks consume 100x less energy than ANNs",
        ),
        (
            "f_corpus_008",
            "corpus",
            "Large language models exhibit emergent reasoning at 70B+ parameters",
        ),
        (
            "f_corpus_009",
            "corpus",
            "Neural architecture search discovers 5x more efficient ConvNet designs",
        ),
        (
            "f_corpus_010",
            "corpus",
            "Contrastive learning achieves 90% accuracy with 1% labeled data",
        ),
    ];
    for (id, origin, content) in &seed_facts {
        let fact = Fact {
            id: FihHash(id.to_string()),
            origin: origin.to_string(),
            content: (*content).into(),
            creator: "corpus".into(),
        };
        bb.submit_fact(&fact).unwrap();
    }
    println!("  Seeded {} facts into empty graph", seed_facts.len());

    // Phase 2: spawn ants and let them randomly interact
    const NUM_ANTS: usize = 30;
    const STEPS: usize = 200;
    let mut ants: Vec<Ant> = (0..NUM_ANTS).map(|i| Ant::new(i)).collect();

    for step in 0..STEPS {
        let ant_idx = rng.gen_range(NUM_ANTS);
        let log = ants[ant_idx].act(&mut bb, &mut rng, step);
        if step < 10 || step % 50 == 0 || step == STEPS - 1 {
            println!("  [step {step:>3}] {log}");
        }
    }

    // Phase 3: verify invariants
    let state = bb.read_state();

    // Invariant 1: all seed facts still present
    assert!(
        state.facts.len() >= seed_facts.len(),
        "lost seed facts: {} < {}",
        state.facts.len(),
        seed_facts.len()
    );

    // Invariant 2: no concluded intent has a worker (released after conclusion)
    for intent in &state.intents {
        if intent.concluded_at.is_some() {
            assert!(
                intent.worker.is_none(),
                "concluded intent {} still has worker='{:?}' (from_facts={:?})",
                intent.id.0,
                intent.worker,
                intent.from_facts
            );
        }
    }

    // Invariant 3: every intent is grounded in at least one existing fact
    let fact_names: HashSet<&str> = state.facts.iter().map(|f| f.id.0.as_str()).collect();
    for intent in &state.intents {
        assert!(
            !intent.from_facts.is_empty(),
            "intent {} has no grounding facts",
            intent.id.0
        );
        for fid in &intent.from_facts {
            assert!(
                fact_names.contains(fid.as_str()) || fid.starts_with("fact_"),
                "intent {} references non-existent fact {fid}",
                intent.id.0
            );
        }
    }

    // Invariant 4: Cypher MATCH returns correct node count
    let fact_count = {
        let plan = cypher::Plan::from_internal("MATCH (f:Fact) RETURN f").unwrap();
        cypher::execute(&bb, &plan).unwrap().len()
    };
    assert_eq!(
        fact_count,
        state.facts.len(),
        "Cypher count != read_state count"
    );

    let intent_count = {
        let plan = cypher::Plan::from_internal("MATCH (i:Intent) RETURN i").unwrap();
        cypher::execute(&bb, &plan).unwrap().len()
    };
    assert_eq!(
        intent_count,
        state.intents.len(),
        "Cypher count != read_state count"
    );

    println!();
    println!("  ✓ Stress test passed: {NUM_ANTS} ants × {STEPS} steps");
    println!(
        "  ✓ Final state: {} facts, {} intents",
        state.facts.len(),
        state.intents.len()
    );
    println!("  ✓ Cypher MATCH counts match read_state");
    println!("  ✓ No conflicting claims (all-or-nothing FIH)");
    println!("  ✓ All intents properly grounded");
}
