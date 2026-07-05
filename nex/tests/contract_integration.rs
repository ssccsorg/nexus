// ── ContractBlackboard integration tests ──────────────────────────────
//
// Tests the governance wrapper over FihStorage, following the same
// structural pattern as FihBlackboard (storage/fih.rs).
//
// ContractBlackboard wraps FihStorage and adds governance gating
// (GovernanceGate admit, HintEngine constraint, EvidenceChain audit).

use std::sync::Arc;

use nex::contract::ContractBlackboard;
use nex::io::{FileIo, IoFuture};
use nex::storage::core::FihStorage;
use nexus_model::fih::{Content, Fact, FihHash};
use nexus_model::{AsyncFactCapable, AsyncIntentCapable, AsyncStorageRead};

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

fn make_contract(enabled: bool) -> ContractBlackboard<TestIo> {
    let storage = Arc::new(FihStorage::new(TestIo, "test-proj"));
    ContractBlackboard::new(storage, enabled)
}

#[test]
fn test_submit_fact_with_gate() {
    let contract = make_contract(true);
    contract.register_schema("text", b"text/plain");

    let fact = Fact {
        id: FihHash::new(&["hello"], "fact"),
        origin: "text".into(),
        content: Content {
            mime_type: "text/plain".into(),
            data: b"hello world".to_vec(),
        },
        creator: "tester".into(),
    };

    let result = futures_executor::block_on(contract.submit_fact(&fact));
    assert!(result.is_ok());
    assert!(contract.evidence_tip().is_some());
}

#[test]
fn test_submit_fact_rejected_by_gate() {
    let contract = make_contract(true);

    let fact = Fact {
        id: FihHash::new(&["data"], "fact"),
        origin: "unknown".into(),
        content: Content {
            mime_type: "text/plain".into(),
            data: b"some data".to_vec(),
        },
        creator: "tester".into(),
    };

    let result = futures_executor::block_on(contract.submit_fact(&fact));
    match result.unwrap_err() {
        nexus_model::BlackboardError::Forbidden(msg) => {
            assert!(msg.contains("not registered"), "msg: {msg}")
        }
        other => panic!("expected Forbidden error, got: {other:?}"),
    }
}

#[test]
fn test_pass_through_when_disabled() {
    let contract = make_contract(false);

    let fact = Fact {
        id: FihHash::new(&["x"], "fact"),
        origin: "unknown".into(),
        content: Content {
            mime_type: "text/plain".into(),
            data: b"data".to_vec(),
        },
        creator: "tester".into(),
    };

    let result = futures_executor::block_on(contract.submit_fact(&fact));
    assert!(result.is_ok());
}

#[test]
fn test_evidence_recorded_on_submit() {
    let contract = make_contract(true);
    contract.register_schema("num", b"i64");

    let fact = Fact {
        id: FihHash::new(&["42"], "fact"),
        origin: "num".into(),
        content: Content::from("42"),
        creator: "test".into(),
    };

    futures_executor::block_on(contract.submit_fact(&fact)).unwrap();
    assert_eq!(contract.evidence.len(), 1);
    assert!(contract.evidence_tip().is_some());
}

#[test]
fn test_hint_engine_integration() {
    let contract = make_contract(true);
    contract.hints.add("positive", nex::contract::HintRule::Positive);

    assert!(contract.check_hints(5).is_ok());
    assert!(contract.check_hints(-1).is_err());
}

#[test]
fn test_is_enabled() {
    assert!(make_contract(true).is_enabled());
    assert!(!make_contract(false).is_enabled());
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
fn test_read_state_pass_through() {
    let contract = make_contract(true);
    let state = futures_executor::block_on(contract.read_state());
    assert_eq!(state.facts.len(), 0);
    assert_eq!(state.intents.len(), 0);
}

#[test]
fn test_project_id() {
    let contract = make_contract(true);
    assert_eq!(contract.project_id(), "test-proj");
}

#[test]
fn test_enabled_constructor() {
    let storage = Arc::new(FihStorage::new(TestIo, "p"));
    let contract = ContractBlackboard::enabled(storage);
    assert!(contract.is_enabled());
}

#[test]
fn test_disabled_constructor() {
    let storage = Arc::new(FihStorage::new(TestIo, "p"));
    let contract = ContractBlackboard::disabled(storage);
    assert!(!contract.is_enabled());
}

#[test]
fn test_verify_evidence() {
    let contract = make_contract(true);
    contract.record_evidence("a", "fact");
    contract.record_evidence("b", "intent");
    assert!(contract.verify_evidence(0));
}

#[test]
fn test_evidence_empty_when_disabled() {
    let contract = make_contract(false);
    assert!(contract.evidence_tip().is_none());
    assert!(contract.evidence.is_empty());
}
