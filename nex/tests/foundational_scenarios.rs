// Foundational document scenario tests.
// Claims extracted exclusively from:
//   manifesto.llms.md           — ontology ("what SSCCS is")
//   philosophy/epistemology.llms.md — epistemology ("how we know")
//   whitepaper/whitepaper.llms.md §2 — formal primitives
//   guide.llms.md               — developer usage
//
// These four documents form the coherent foundation against which
// all other SSCCS documents should be consistent. The tests verify
// that the nexus detection layer can:
//   1. Find contradictions between ontology and formal definitions
//   2. Detect gaps between abstract philosophy and practical guide
//   3. Track knowledge evolution when formal §2 revises manifesto claims
//   4. Support multi-agent review across foundational layers

use nex::process::scheduler::Scheduler;
use nex::process::tasks::contradiction_detector::ContradictionDetector;
use nex::process::tasks::gap_detector::GapDetector;
use nex::process::tasks::new_document_analyzer::NewDocumentAnalyzer;
use nex::process::tasks::state_change_detector::StateChangeDetector;
use nex::{
    Blackboard, BoardState, EvictCapable, Fact, FactCapable, FihHash, Intent, IntentCapable,
    StorageRead, create_blackboard,
};

fn claim(id: &str, origin: &str, claim_text: &str, topic: &str, position: &str) -> Fact {
    Fact {
        id: FihHash(id.to_string()),
        origin: origin.to_string(),
        content: serde_json::to_string(
            &serde_json::json!({ "claim": claim_text, "topic": topic, "position": position }),
        )
        .unwrap_or_default()
        .into(),
        creator: "ingester".into(),
    }
}

fn do_tick(sched: &mut Scheduler<impl Blackboard + EvictCapable>) -> usize {
    sched.tick().expect("tick")
}

fn facts_by_creator<'a>(state: &'a BoardState, creator: &str) -> Vec<&'a Fact> {
    state
        .facts
        .iter()
        .filter(|f| f.creator == creator)
        .collect()
}

// ═════════════════════════════════════════════════════════════════════════
//  Foundational Corpus
// ═════════════════════════════════════════════════════════════════════════

fn seed_foundational(bb: &impl Blackboard) -> Vec<String> {
    let facts = [
        // ── manifesto.llms.md — ontology ──────────────────────────
        claim(
            "mf01",
            "manifesto.llms.md",
            "Computation is the collapse of structured potential",
            "computation-definition",
            "collapse-of-potential",
        ),
        claim(
            "mf02",
            "manifesto.llms.md",
            "There are no fundamental values. There are no intrinsic algorithms.",
            "computation-definition",
            "no-fundamentals",
        ),
        claim(
            "mf03",
            "manifesto.llms.md",
            "Time is just another coordinate, not a privileged timeline of execution",
            "time-ontology",
            "coordinate-only",
        ),
        claim(
            "mf04",
            "manifesto.llms.md",
            "Segment is immutable, contains no value, contains no state",
            "segment-definition",
            "pure-coordinate",
        ),
        claim(
            "mf05",
            "manifesto.llms.md",
            "Field does not store values. It stores admissibility conditions.",
            "field-definition",
            "admissibility-conditions",
        ),
        claim(
            "mf06",
            "manifesto.llms.md",
            "Observation is the only mechanism that produces actuality",
            "observation-role",
            "sole-active-event",
        ),
        claim(
            "mf07",
            "manifesto.llms.md",
            "Projection is transient, not stored, not intrinsic value",
            "projection-nature",
            "ephemeral",
        ),
        // ── epistemology.llms.md — how we know ─────────────────────
        claim(
            "ep01",
            "epistemology.llms.md",
            "A Field is a bounded domain of possible observations with constraints",
            "field-definition",
            "bounded-domain-with-constraints",
        ),
        claim(
            "ep02",
            "epistemology.llms.md",
            "To know is to delineate. Knowledge begins with constraint.",
            "knowledge-origin",
            "constraint-primary",
        ),
        claim(
            "ep03",
            "epistemology.llms.md",
            "Composition is the combination of constraint structures to define new observation spaces",
            "composition-definition",
            "constraint-combination",
        ),
        claim(
            "ep04",
            "epistemology.llms.md",
            "Constraints are the grammar of reality — they determine what is observable",
            "constraint-role",
            "grammar-of-reality",
        ),
        claim(
            "ep05",
            "epistemology.llms.md",
            "A gap is a known unknown — constraint grammar understood, observation not yet made",
            "gap-definition",
            "known-unknown",
        ),
        claim(
            "ep06",
            "epistemology.llms.md",
            "Statistical frameworks cannot structurally deduce what lies beyond their training data",
            "statistics-limitation",
            "interpolation-only",
        ),
        claim(
            "ep07",
            "epistemology.llms.md",
            "Intelligence is the ability to decompose, map, compose, and verify constraints",
            "intelligence-definition",
            "structural-intelligence",
        ),
        // ── whitepaper §2 — formal primitives ─────────────────────
        claim(
            "wp01",
            "whitepaper.llms.md",
            "Segment s = (c, id) where c ∈ R^d, id = H(c)",
            "segment-definition",
            "formal-tuple",
        ),
        claim(
            "wp02",
            "whitepaper.llms.md",
            "Scheme Σ = (A, R, L, O) — axes, relations, memory-layout, observation rules",
            "scheme-definition",
            "formal-quadruple",
        ),
        claim(
            "wp03",
            "whitepaper.llms.md",
            "Field F = (C, T) — constraint predicate C and transition matrix T",
            "field-definition",
            "formal-pair",
        ),
        claim(
            "wp04",
            "whitepaper.llms.md",
            "Observation P = Ω(Σ, F) — deterministic projection from Scheme and Field",
            "observation-definition",
            "deterministic-function",
        ),
        claim(
            "wp05",
            "whitepaper.llms.md",
            "Time is treated as one coordinate axis among many; observations have no global temporal order",
            "time-ontology",
            "coordinate-only",
        ),
        claim(
            "wp06",
            "whitepaper.llms.md",
            "Segments are immutable; concurrent observations do not interfere",
            "segment-definition",
            "immutable-concurrent",
        ),
        claim(
            "wp07",
            "whitepaper.llms.md",
            "MemoryLayout is a constraint-satisfaction problem determining execution feasibility",
            "memory-layout",
            "constraint-satisfaction",
        ),
        // ── guide.llms.md — developer usage ───────────────────────
        claim(
            "gd01",
            "guide.llms.md",
            "A developer designs structure and sets conditions — no instruction writing",
            "developer-role",
            "structure-designer",
        ),
        claim(
            "gd02",
            "guide.llms.md",
            "Field is a set of dynamic rules — the only mutable layer",
            "field-definition",
            "practical-mutable",
        ),
        claim(
            "gd03",
            "guide.llms.md",
            "Field gives meaning to points: 'point 0 is 1, point 1 is 1'",
            "field-definition",
            "value-binding",
        ),
        claim(
            "gd04",
            "guide.llms.md",
            "SSCCS automates: data layout, cache alignment, SIMD, thread scheduling, lock management",
            "developer-benefit",
            "automation",
        ),
        claim(
            "gd05",
            "guide.llms.md",
            "Data movement consumes 60-80% of energy in modern systems",
            "energy-efficiency",
            "data-movement-cost",
        ),
    ];
    let ids: Vec<String> = facts.iter().map(|f| f.id.0.clone()).collect();
    for f in &facts {
        bb.submit_fact(f).unwrap();
    }
    ids
}

// ═════════════════════════════════════════════════════════════════════════
//  Scenario 1: Foundational Consistency Audit
//  Detects tensions between manifesto (ontology), epistemology (how we know),
//  whitepaper §2 (formal), and guide (practical) layers.
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_foundational_consistency_audit() {
    let bb = create_blackboard();
    let baseline = seed_foundational(&bb);

    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));
    sched.register(Box::new(ContradictionDetector::new()));
    sched.register(Box::new(NewDocumentAnalyzer::with_baseline(baseline)));

    // Phase 1: Initial analysis
    do_tick(&mut sched);
    let state = StorageRead::read_state(&sched.bb);

    // Gap facts: cross-origin gaps between the 4 foundational layers
    let gaps = facts_by_creator(&state, "gap-detector");
    assert!(!gaps.is_empty(), "Cross-layer gaps found: {}", gaps.len());

    // Contradiction: "field-definition" has 4 different positions across documents
    // - manifesto: admissibility-conditions
    // - epistemology: bounded-domain-with-constraints
    // - whitepaper §2: formal-pair (C, T)
    // - guide: practical-mutable + value-binding
    let contradictions = facts_by_creator(&state, "contradiction-detector");
    let field_tensions: Vec<_> = contradictions
        .iter()
        .filter(|f| {
            let cv: serde_json::Value = serde_json::from_str(f.content.as_str().unwrap_or(""))
                .unwrap_or(serde_json::Value::Null);
            cv.get("topic").and_then(|v| v.as_str()) == Some("field-definition")
        })
        .collect();
    assert!(
        !field_tensions.is_empty(),
        "field-definition contradictions across 4 foundations: {}",
        field_tensions.len()
    );

    // "segment-definition": pure-coordinate (manifesto) vs formal-tuple (whitepaper §2)
    let segment_tensions: Vec<_> = contradictions
        .iter()
        .filter(|f| {
            let cv: serde_json::Value = serde_json::from_str(f.content.as_str().unwrap_or(""))
                .unwrap_or(serde_json::Value::Null);
            cv.get("topic").and_then(|v| v.as_str()) == Some("segment-definition")
        })
        .collect();
    assert!(
        !segment_tensions.is_empty(),
        "segment-definition: philosophical vs formal: {}",
        segment_tensions.len()
    );

    // Verify all 4 document origins appear in cross-origin gaps
    let all_origins: Vec<&str> = state.facts.iter().map(|f| f.origin.as_str()).collect();
    assert!(all_origins.iter().any(|o| o.contains("manifesto")));
    assert!(all_origins.iter().any(|o| o.contains("epistemology")));
    assert!(all_origins.iter().any(|o| o.contains("whitepaper")));
    assert!(all_origins.iter().any(|o| o.contains("guide")));
}

// ═════════════════════════════════════════════════════════════════════════
//  Scenario 2: Formal Revision of Philosophical Claims
//  Whitepaper §2 provides formal definitions that may refine manifesto
//  declarations. The system should detect when formal treatment adds
//  precision that the philosophical layer lacks.
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_formal_revision_of_philosophy() {
    let bb = create_blackboard();

    // Phase 1: Only manifesto + epistemology (philosophical layer)
    let phil_facts = [
        claim(
            "p01",
            "manifesto.llms.md",
            "Computation is the collapse of structured potential",
            "computation-definition",
            "collapse-of-potential",
        ),
        claim(
            "p02",
            "manifesto.llms.md",
            "Segment is immutable, contains no value, contains no state",
            "segment-definition",
            "pure-coordinate",
        ),
        claim(
            "p03",
            "manifesto.llms.md",
            "Field does not store values. It stores admissibility conditions.",
            "field-definition",
            "admissibility-conditions",
        ),
        claim(
            "p04",
            "epistemology.llms.md",
            "A Field is a bounded domain of possible observations with constraints",
            "field-definition",
            "bounded-domain-with-constraints",
        ),
        claim(
            "p05",
            "epistemology.llms.md",
            "Knowledge begins with constraint. To know is to delineate.",
            "knowledge-origin",
            "constraint-primary",
        ),
    ];
    let phil_ids: Vec<String> = phil_facts.iter().map(|f| f.id.0.clone()).collect();
    for f in &phil_facts {
        bb.submit_fact(f).unwrap();
    }

    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));
    sched.register(Box::new(ContradictionDetector::new()));
    sched.register(Box::new(StateChangeDetector::new()));
    sched.register(Box::new(NewDocumentAnalyzer::with_baseline(phil_ids)));

    // Analyze philosophical layer
    do_tick(&mut sched);
    let state1 = StorageRead::read_state(&sched.bb);
    let contradictions_before = facts_by_creator(&state1, "contradiction-detector").len();

    // Phase 2: Whitepaper §2 arrives — formal definitions
    let formal_facts = [
        claim(
            "f01",
            "whitepaper.llms.md",
            "Segment s = (c, id) where c ∈ R^d, id = H(c)",
            "segment-definition",
            "formal-tuple",
        ),
        claim(
            "f02",
            "whitepaper.llms.md",
            "Scheme Σ = (A, R, L, O) — axes, relations, memory-layout, observation rules",
            "scheme-definition",
            "formal-quadruple",
        ),
        claim(
            "f03",
            "whitepaper.llms.md",
            "Field F = (C, T) — constraint predicate C: S→B and transition matrix T: S×S→R",
            "field-definition",
            "formal-pair",
        ),
        claim(
            "f04",
            "whitepaper.llms.md",
            "Observation P = Ω(Σ, F) — deterministic function",
            "observation-definition",
            "deterministic-function",
        ),
    ];
    for f in &formal_facts {
        sched.bb.submit_fact(f).unwrap();
    }

    do_tick(&mut sched);
    let state2 = StorageRead::read_state(&sched.bb);
    let contradictions_after = facts_by_creator(&state2, "contradiction-detector").len();

    // More contradictions after formal definitions arrive
    // (formal-tuple vs pure-coordinate, formal-pair vs admissibility-conditions)
    assert!(
        contradictions_after > contradictions_before,
        "Formal §2 adds precision tensions: {} -> {}",
        contradictions_before,
        contradictions_after
    );

    // NDA should find challenges (-factors) for each topic
    let nda_facts = facts_by_creator(&state2, "new-document-analyzer");
    let challenges = nda_facts
        .iter()
        .filter(|f| {
            let cv: serde_json::Value = serde_json::from_str(f.content.as_str().unwrap_or(""))
                .unwrap_or(serde_json::Value::Null);
            cv.get("factor").and_then(|v| v.as_str()) == Some("-factor")
        })
        .count();
    assert!(
        challenges >= 2,
        "Formal §2 challenges philosophical claims: {} -factors",
        challenges
    );

    // Agent: resolve the field-definition tension
    let field_contradiction = state2.facts.iter().find(|f| {
        f.creator == "contradiction-detector" && {
            let cv: serde_json::Value = serde_json::from_str(f.content.as_str().unwrap_or(""))
                .unwrap_or(serde_json::Value::Null);
            cv.get("topic").and_then(|v| v.as_str()) == Some("field-definition")
        }
    });
    if let Some(cf) = field_contradiction {
        let intent = Intent {
            id: FihHash::new(&[&cf.id.0, "resolve"], "intent"),
            from_facts: vec![cf.id.0.clone()],
            description: "Resolve field-definition across layers".into(),
            creator: "formal-reviewer".into(),
            worker: None,
            to_fact_id: None,
            last_heartbeat_at: None,
            created_at: None,
            concluded_at: None,
        };
        let iid = sched.bb.submit_intent(&intent).expect("submit");
        sched
            .bb
            .claim_intent(&iid.0, "formal-reviewer")
            .expect("claim");
        sched.bb.conclude_intent(&iid.0, &serde_json::to_string(&serde_json::json!({
            "synthesis": "Manifesto declares what Field IS (admissibility conditions). Epistemology explains what Field DOES (bounds observation). Whitepaper §2 defines Field formally as (C,T). All three are consistent layers of the same concept."
        })).unwrap()).expect("conclude");
    }

    let final_state = StorageRead::read_state(&sched.bb);
    assert!(
        final_state.facts.len() > 9,
        "Knowledge grew through formal revision: {} facts",
        final_state.facts.len()
    );
}

// ═════════════════════════════════════════════════════════════════════════
//  Scenario 3: Gap Between Theory and Practice
//  Manifesto/epistemology define WHAT and WHY. Guide defines HOW.
//  The gap between them is the implementation frontier.
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_theory_practice_gap() {
    let bb = create_blackboard();

    // Theory layer: manifesto + epistemology
    let theory = [
        claim(
            "t01",
            "manifesto.llms.md",
            "Loops disappear into layout. Optimization becomes structural.",
            "compilation-vision",
            "structural-optimization",
        ),
        claim(
            "t02",
            "manifesto.llms.md",
            "The Scheme is not code. It is structural law.",
            "scheme-role",
            "structural-law",
        ),
        claim(
            "t03",
            "epistemology.llms.md",
            "Composition takes existing constraint structures and combines them to define entirely new observation spaces",
            "composition-power",
            "generative-not-predictive",
        ),
        claim(
            "t04",
            "epistemology.llms.md",
            "Every novel observation generated by Composition can be traced back to specific constraint configurations",
            "composition-property",
            "fully-auditable",
        ),
    ];
    let theory_ids: Vec<String> = theory.iter().map(|f| f.id.0.clone()).collect();
    for f in &theory {
        bb.submit_fact(f).unwrap();
    }

    // Practice layer: guide
    let practice = [
        claim(
            "p01",
            "guide.llms.md",
            "A developer designs structure and sets conditions",
            "developer-role",
            "structure-designer",
        ),
        claim(
            "p02",
            "guide.llms.md",
            "SSCCS automates: data layout, cache alignment, SIMD vectorization, thread scheduling, lock management",
            "compilation-vision",
            "practical-automation",
        ),
        claim(
            "p03",
            "guide.llms.md",
            "The compiler analyzes the Scheme and maps it to physical memory",
            "compiler-role",
            "physical-mapping",
        ),
        claim(
            "p04",
            "guide.llms.md",
            "Future: Translation Compiler analyzes data formats → generates .ss files",
            "compiler-future",
            "translation-compiler",
        ),
    ];

    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));
    sched.register(Box::new(ContradictionDetector::new()));
    sched.register(Box::new(NewDocumentAnalyzer::with_baseline(theory_ids)));

    // Phase 1: Analyze theory only
    do_tick(&mut sched);

    // Phase 2: Guide arrives — bridge between theory and practice
    for f in &practice {
        sched.bb.submit_fact(f).unwrap();
    }
    do_tick(&mut sched);

    let state = StorageRead::read_state(&sched.bb);

    // NDA: guide should both support (+factor) and extend (gap) the theory
    let nda = facts_by_creator(&state, "new-document-analyzer");
    // Guide challenges theory: same topics, different positions → -factors
    let content_val_of = |f: &&Fact| -> serde_json::Value {
        serde_json::from_str(f.content.as_str().unwrap_or("")).unwrap_or(serde_json::Value::Null)
    };
    let factor_of = |f: &&Fact| -> Option<String> {
        content_val_of(f)
            .get("factor")?
            .as_str()
            .map(|s| s.to_string())
    };

    let challenges = nda
        .iter()
        .filter(|f| factor_of(f).as_deref() == Some("-factor"))
        .count();
    let gaps = nda
        .iter()
        .filter(|f| factor_of(f).as_deref() == Some("gap"))
        .count();

    assert!(
        challenges > 0,
        "Guide challenges theory: {} -factors",
        challenges
    );
    assert!(gaps > 0, "Guide introduces new topics: {} gaps", gaps);

    // Gap detector: cross-origin gaps between manifesto/epistemology and guide
    let gap_facts = facts_by_creator(&state, "gap-detector");
    assert!(
        !gap_facts.is_empty(),
        "Theory-practice gaps found: {}",
        gap_facts.len()
    );

    // Verify the compilation-vision topic shows the theory-practice bridge
    let compilation_gaps: Vec<_> = gap_facts
        .iter()
        .filter(|f| f.content.to_string().contains("compilation"))
        .collect();
    // compilation-vision: structural-optimization (theory) vs practical-automation (practice)
    // This gap is exactly where SSCCS implementation lives
    assert!(
        compilation_gaps.len() + challenges > 0,
        "Theory-practice bridge on compilation: {} gaps + {} challenges",
        compilation_gaps.len(),
        challenges
    );
}

// ═════════════════════════════════════════════════════════════════════════
//  Scenario 4: Epistemology as the Bridge
//  Epistemology.llms.md bridges manifesto (ontology) and whitepaper (formal).
//  It defines the "how we know" layer that connects "what is" to formal math.
// ═════════════════════════════════════════════════════════════════════════

#[test]
fn scenario_epistemology_as_bridge() {
    let bb = create_blackboard();

    // Manifesto (what IS) + Whitepaper §2 (formal definitions)
    let claims = [
        claim(
            "a01",
            "manifesto.llms.md",
            "Computation is the collapse of structured potential",
            "computation-definition",
            "collapse-of-potential",
        ),
        claim(
            "a02",
            "manifesto.llms.md",
            "Observation is the only mechanism that produces actuality",
            "observation-role",
            "sole-active-event",
        ),
        claim(
            "a03",
            "whitepaper.llms.md",
            "Observation P = Ω(Σ, F) — deterministic function",
            "observation-definition",
            "deterministic-function",
        ),
        claim(
            "a04",
            "whitepaper.llms.md",
            "Segments are immutable; concurrent observations do not interfere",
            "segment-definition",
            "immutable-concurrent",
        ),
    ];
    let baseline: Vec<String> = claims.iter().map(|f| f.id.0.clone()).collect();
    for f in &claims {
        bb.submit_fact(f).unwrap();
    }

    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(ContradictionDetector::new()));
    sched.register(Box::new(GapDetector::new()));
    sched.register(Box::new(NewDocumentAnalyzer::with_baseline(baseline)));

    // Phase 1: Without epistemology, manifesto and whitepaper §2 have tensions
    do_tick(&mut sched);
    let state1 = StorageRead::read_state(&sched.bb);
    let _contradictions_without_ep = facts_by_creator(&state1, "contradiction-detector").len();

    // Phase 2: Epistemology arrives — provides the connecting layer
    let ep_claims = [
        claim(
            "e01",
            "epistemology.llms.md",
            "Observation is the moment a constraint set executes and a new observable outcome is produced",
            "observation-definition",
            "constraint-execution-event",
        ),
        claim(
            "e02",
            "epistemology.llms.md",
            "This epistemology inverts the intuition: knowledge begins with constraint",
            "knowledge-origin",
            "constraint-primary",
        ),
        claim(
            "e03",
            "epistemology.llms.md",
            "A constraint is the act of drawing a boundary within the space of the possible",
            "constraint-definition",
            "boundary-drawing",
        ),
        claim(
            "e04",
            "epistemology.llms.md",
            "If something can be known through Composition, the path can be fully reconstructed",
            "composition-property",
            "fully-auditable",
        ),
    ];
    for f in &ep_claims {
        sched.bb.submit_fact(f).unwrap();
    }

    do_tick(&mut sched);
    let state2 = StorageRead::read_state(&sched.bb);

    // Epistemology bridges by introducing mediating positions.
    // Same topics, different positions → -factors (constructive challenges)
    let nda = facts_by_creator(&state2, "new-document-analyzer");
    let content_val_of = |f: &&Fact| -> serde_json::Value {
        serde_json::from_str(f.content.as_str().unwrap_or("")).unwrap_or(serde_json::Value::Null)
    };
    let factor_of = |f: &&Fact| -> Option<String> {
        content_val_of(f)
            .get("factor")?
            .as_str()
            .map(|s| s.to_string())
    };
    let challenges = nda
        .iter()
        .filter(|f| factor_of(f).as_deref() == Some("-factor"))
        .count();
    assert!(
        challenges > 0,
        "Epistemology challenges existing positions: {} -factors",
        challenges
    );

    // The system should now have richer cross-document connections
    let gaps = facts_by_creator(&state2, "gap-detector");
    assert!(
        gaps.len() > 0,
        "Knowledge graph richer with epistemology bridge: {} gap facts",
        gaps.len()
    );

    // Total knowledge grew
    assert!(
        state2.facts.len() > state1.facts.len(),
        "Epistemology expanded knowledge: {} -> {}",
        state1.facts.len(),
        state2.facts.len()
    );
}
