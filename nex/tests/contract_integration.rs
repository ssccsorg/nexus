// ── FihContract integration tests ─────────────────────────────────────
//
// Tests the full FihContract wrapper with simulated IO.
// Uses futures_executor::block_on to drive async storage operations.

use std::sync::Arc;

use nex::contract::{FihContract, HintRule};
use nex::io::{FileIo, IoFuture};
use nex::storage::core::FihStorage;
use nexus_model::error::BlackboardError;
use nexus_model::fih::{Content, Fact, FihHash, Intent};

struct TestIo;

impl FileIo for TestIo {
    fn read<'a>(&'a self, _path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        Box::pin(async { Ok(None) })
    }
    fn write<'a>(&'a self, _path: &'a str, _data: &'a [u8]) -> IoFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn list<'a>(&'a self, _prefix: &'a str) -> IoFuture<'a, Vec<String>> {
        Box::pin(async { Ok(vec![]) })
    }
    fn delete<'a>(&'a self, _path: &'a str) -> IoFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}

fn make_contract(enabled: bool) -> FihContract<TestIo> {
    let storage = Arc::new(FihStorage::new(TestIo, "test-proj"));
    FihContract::new(storage, enabled)
}

#[test]
fn test_submit_fact_with_gate() {
    let contract = make_contract(true);
    contract.gate.register_schema("text", b"text/plain");

    let fact = Fact {
        id: FihHash::new(&["hello"], "fact"),
        origin: "test".into(),
        content: Content {
            mime_type: "text/plain".into(),
            data: b"hello world".to_vec(),
        },
        creator: "tester".into(),
    };

    let result = futures_executor::block_on(contract.submit_fact(&fact, "text"));
    assert!(result.is_ok());
    assert!(!contract.evidence.is_empty());
}

#[test]
fn test_submit_fact_rejected_by_gate() {
    let contract = make_contract(true);

    let fact = Fact {
        id: FihHash::new(&["data"], "fact"),
        origin: "test".into(),
        content: Content {
            mime_type: "text/plain".into(),
            data: b"some data".to_vec(),
        },
        creator: "tester".into(),
    };

    let result = futures_executor::block_on(contract.submit_fact(&fact, "unknown"));
    match result.unwrap_err() {
        BlackboardError::Forbidden(msg) => assert!(msg.contains("not registered")),
        other => panic!("expected Forbidden error, got: {other:?}"),
    }
}

#[test]
fn test_pass_through_when_disabled() {
    let contract = make_contract(false);

    let fact = Fact {
        id: FihHash::new(&["x"], "fact"),
        origin: "test".into(),
        content: Content {
            mime_type: "text/plain".into(),
            data: b"data".to_vec(),
        },
        creator: "tester".into(),
    };

    let result = futures_executor::block_on(contract.submit_fact(&fact, "unknown"));
    assert!(result.is_ok());
}

#[test]
fn test_evidence_recorded_on_submit() {
    let contract = make_contract(true);
    contract.gate.register_schema("num", b"i64");

    let fact = Fact {
        id: FihHash::new(&["42"], "fact"),
        origin: "test".into(),
        content: Content::from("42"),
        creator: "test".into(),
    };

    futures_executor::block_on(contract.submit_fact(&fact, "num")).unwrap();
    assert_eq!(contract.evidence.len(), 1);
    assert!(contract.evidence.tip().is_some());
}

#[test]
fn test_hint_engine_integration() {
    let contract = make_contract(true);
    contract.hints.add("positive", HintRule::Positive);

    assert!(contract.check_hints(5).is_ok());
    assert!(contract.check_hints(-1).is_err());
}

#[test]
fn test_disabled_contract() {
    let contract = make_contract(false);
    assert!(!contract.is_enabled());
}

#[test]
fn test_toggle_contract_runtime() {
    let mut contract = make_contract(false);
    assert!(!contract.is_enabled());
    contract.set_enabled(true);
    assert!(contract.is_enabled());
    contract.set_enabled(false);
    assert!(!contract.is_enabled());
}

#[test]
fn test_unchecked_bypass() {
    let contract = make_contract(true);

    let fact = Fact {
        id: FihHash::new(&["bypass"], "fact"),
        origin: "test".into(),
        content: Content::from("bypass data"),
        creator: "test".into(),
    };

    let result = futures_executor::block_on(contract.submit_fact_unchecked(&fact));
    assert!(result.is_ok());
}

#[test]
fn test_manual_evidence() {
    let contract = make_contract(true);
    assert!(contract.evidence.is_empty());

    contract.record_evidence("custom-action", "custom-type");
    assert_eq!(contract.evidence.len(), 1);

    let entry = contract.evidence.get(0).unwrap();
    assert_eq!(entry.action_hash, "custom-action");
    assert_eq!(entry.action_type, "custom-type");
}

#[test]
fn test_clear_hints() {
    let contract = make_contract(true);
    contract.hints.add("h1", HintRule::Gt(5));
    contract.hints.add("h2", HintRule::Lt(100));
    assert_eq!(contract.hints.len(), 2);

    contract.clear_hints();
    assert!(contract.hints.is_empty());
}

#[test]
fn test_project_id() {
    let contract = make_contract(true);
    assert_eq!(contract.project_id(), "test-proj");
}
