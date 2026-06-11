// Gap coverage tests for NativeFihStorage<SimFihIo>.
// Tests basic storage invariants not covered by existing scenario tests.

mod common;

use nexus_model::{FactCapable, FihHash, Hint, HintCapable, IntentCapable, StorageRead};
use nexus_storage_sim::{NativeFihStorage, SimFihIo};

fn store() -> NativeFihStorage<SimFihIo> {
    NativeFihStorage::new(SimFihIo::new(), "test")
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
        id: FihHash("h001".into()),
        content: "test hint".into(),
        creator: "tester".into(),
    };
    s.submit_hint(&hint).unwrap();
    let state = s.read_state();
    assert_eq!(state.hints.len(), 1);
    assert_eq!(state.hints[0].content, "test hint");
}
