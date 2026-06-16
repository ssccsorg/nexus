// Gap coverage tests for FihStorage<SimIo>.
// Tests basic storage invariants not covered by existing scenario tests.

mod common;

use nexus_model::{FactCapable, FihHash, Hint, HintCapable, IntentCapable, StorageRead};
use nexus_storage_sim::{FihStorage, SimIo};

fn store() -> FihStorage<SimIo> {
    FihStorage::new(SimIo::new(), "test")
}

#[test]
fn test_empty_state() {
    let s = store();
    let state = s.read_state();
    assert_eq!(state.facts.len(), 0);
    assert_eq!(state.intents.len(), 0);
    assert_eq!(state.hints.len(), 0);
}

#[test]
fn test_duplicate_fact_same_id_replaces() {
    let s = store();
    let f1 = common::fact("f001");
    s.submit_fact(&f1).unwrap();
    // Submitting the same fact ID again replaces the old one (HashMap semantics)
    let f2 = common::fact("f001");
    s.submit_fact(&f2).unwrap();
    let state = s.read_state();
    assert_eq!(state.facts.len(), 1, "same fact ID replaces previous entry");
}

#[test]
fn test_multiple_intents_same_fact_refcount() {
    let s = store();
    s.submit_fact(&common::fact("f_base")).unwrap();
    s.submit_intent(&common::intent("i_a", vec!["f_base"]))
        .unwrap();
    s.submit_intent(&common::intent("i_b", vec!["f_base"]))
        .unwrap();

    // Both intents exist
    let state = s.read_state();
    assert_eq!(state.intents.len(), 2);

    // Conclude both — each creates a conclusion fact
    s.claim_intent("i_a", "agent").unwrap();
    s.conclude_intent("i_a", "done").unwrap();
    s.claim_intent("i_b", "agent").unwrap();
    s.conclude_intent("i_b", "done").unwrap();

    // The original fact plus two conclusion facts (one per intent)
    let state = s.read_state();
    assert_eq!(
        state.facts.len(),
        3,
        "f_base + conclusion for i_a + conclusion for i_b"
    );
}

#[test]
fn test_intent_not_found_error() {
    let s = store();
    let result = s.claim_intent("nonexistent", "agent");
    assert!(result.is_err());
    let result = s.heartbeat("nonexistent", "agent");
    assert!(result.is_err());
    let result = s.conclude_intent("nonexistent", "done");
    assert!(result.is_err());
}

#[test]
fn test_bulk_fact_submission() {
    let s = store();
    for i in 0..1000 {
        s.submit_fact(&common::fact(&format!("f{:04}", i))).unwrap();
    }
    let state = s.read_state();
    assert_eq!(state.facts.len(), 1000);
}

#[test]
fn test_submit_hint_then_read() {
    let s = store();
    let hint = Hint {
        id: FihHash::from_hex("h001"),
        content: "test hint".into(),
        creator: "tester".into(),
    };
    s.submit_hint(&hint).unwrap();
    let state = s.read_state();
    assert_eq!(state.hints.len(), 1);
    assert_eq!(state.hints[0].content, "test hint");
}

/// Minimal functionality test: submit facts 1,2,3, intents a,b,c referencing
/// them, and a hint with random string content. Verify all stored and readable.
///
/// This is the simplest possible "does the whole thing work" test — no flush,
/// no rebuild, no time travel. Just submit and read.
#[test]
fn test_minimal_fih_lifecycle() {
    let s = store();

    // Three facts
    for id in &["f_1", "f_2", "f_3"] {
        s.submit_fact(&common::fact(id)).unwrap();
    }

    // Three intents referencing facts in various combinations
    s.submit_intent(&common::intent("i_a", vec!["f_1"]))
        .unwrap();
    s.submit_intent(&common::intent("i_b", vec!["f_2"]))
        .unwrap();
    s.submit_intent(&common::intent("i_c", vec!["f_1", "f_3"]))
        .unwrap();

    // A hint with arbitrary string content
    s.submit_hint(&Hint {
        id: FihHash::from_hex("h_guide"),
        content: "random constraint string: xkcd-934".into(),
        creator: "tester".into(),
    })
    .unwrap();

    // Verify everything stored
    let state = s.read_state();
    assert_eq!(state.facts.len(), 3, "facts 1,2,3");
    assert_eq!(state.intents.len(), 3, "intents a,b,c");
    assert_eq!(state.hints.len(), 1, "one hint");

    // Verify reverse index: which intents reference f_1?
    let refs = s.intents_by_fact("f_1");
    assert_eq!(refs.len(), 2, "f_1 referenced by i_a and i_c");
    assert!(refs.contains(&"i_a".to_string()));
    assert!(refs.contains(&"i_c".to_string()));

    // Verify non-referenced fact has empty reverse index
    assert!(s.intents_by_fact("f_2").len() == 1);
    assert!(s.intents_by_fact("nonexistent").is_empty());
}

/// Minimal lifecycle with claim → conclude: verify state machine works.
#[test]
fn test_minimal_claim_conclude() {
    let s = store();

    s.submit_fact(&common::fact("f_target")).unwrap();
    s.submit_intent(&common::intent("i_work", vec!["f_target"]))
        .unwrap();

    s.claim_intent("i_work", "agent").unwrap();
    let state = s.read_state();
    assert_eq!(state.intents[0].worker.as_deref(), Some("agent"));

    s.conclude_intent("i_work", "result achieved").unwrap();
    let state = s.read_state();
    assert_eq!(state.facts.len(), 2, "original + conclusion fact");
    assert!(state.intents[0].is_concluded);
}
