// Integration test: OODA loop with gap detector.
//
// Validates that the scheduler + gap detector work together:
//   1. Seed orphaned facts into DefaultBlackboard
//   2. Run the scheduler with gap detector registered
//   3. Verify the gap detector submits Intents for orphaned facts

use nexus_graph::{create_blackboard, Blackboard, Fact, FihHash};
use nexus_process::scheduler::Scheduler;
use nexus_process::tasks::gap_detector::GapDetector;

#[test]
fn test_gap_detector_creates_intents_for_orphaned_facts() {
    let mut bb = create_blackboard();

    // Seed two facts from the same origin (both orphaned — no intent references them)
    bb.submit_fact(&Fact {
        id: FihHash("f_alpha".into()),
        origin: "sensor-a".into(),
        content: serde_json::json!("temperature spike"),
        creator: "tester".into(),
    })
    .unwrap();
    bb.submit_fact(&Fact {
        id: FihHash("f_beta".into()),
        origin: "sensor-a".into(),
        content: serde_json::json!("pressure drop"),
        creator: "tester".into(),
    })
    .unwrap();

    // One fact from a different origin (alone, should not trigger synthesis)
    bb.submit_fact(&Fact {
        id: FihHash("f_gamma".into()),
        origin: "sensor-b".into(),
        content: serde_json::json!("humidity normal"),
        creator: "tester".into(),
    })
    .unwrap();

    // Create scheduler with gap detector
    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));

    // Run one OODA tick
    let submitted = sched.tick().expect("tick should succeed");
    assert_eq!(
        submitted, 1,
        "gap detector should submit 1 intent for sensor-a"
    );

    // Verify the intent appears in board state
    let state = Blackboard::read_state(&sched.bb);
    assert_eq!(state.intents.len(), 1, "exactly 1 intent should exist");
    assert_eq!(
        state.intents[0].from_facts.len(),
        2,
        "intent should reference both sensor-a facts"
    );
    assert!(state.intents[0].description.contains("Synthesise"));
    assert!(state.intents[0].description.contains("sensor-a"));
}

#[test]
fn test_gap_detector_no_orphans_no_intents() {
    let bb = create_blackboard();

    // No facts — gap detector should not create intents
    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));
    let submitted = sched.tick().expect("tick should succeed");
    assert_eq!(submitted, 0, "no intent for empty board");
}

#[test]
fn test_scheduler_multiple_ticks() {
    let mut bb = create_blackboard();
    bb.submit_fact(&Fact {
        id: FihHash("f_a".into()),
        origin: "src".into(),
        content: serde_json::json!("a"),
        creator: "t".into(),
    })
    .unwrap();
    bb.submit_fact(&Fact {
        id: FihHash("f_b".into()),
        origin: "src".into(),
        content: serde_json::json!("b"),
        creator: "t".into(),
    })
    .unwrap();

    let mut sched = Scheduler::new(bb);
    sched.register(Box::new(GapDetector::new()));

    // First tick: submits intent
    assert_eq!(sched.tick().unwrap(), 1, "first tick submits intent");

    // Second tick: gap detector sees no new orphans
    assert_eq!(sched.tick().unwrap(), 0, "second tick: no more orphans");
}
