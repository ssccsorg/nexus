// ── FihStorage governance integration tests ───────────────────────────
//
// Tests the governance layer assembled onto FihStorage.
// Governance is activated via FihStorage::with_governance().

use std::sync::Arc;

use nex::io::{FileIo, IoFuture};
use nex::storage::core::FihStorage;
use nexus_model::fih::{Content, Fact, FihHash, Intent};
use nexus_model::AsyncFactCapable;

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

fn make_storage() -> Arc<FihStorage<TestIo>> {
    Arc::new(FihStorage::with_governance(TestIo, "test-proj"))
}

fn make_storage_disabled() -> Arc<FihStorage<TestIo>> {
    Arc::new(FihStorage::with_governance_disabled(TestIo, "test-proj"))
}

fn make_storage_no_gov() -> Arc<FihStorage<TestIo>> {
    Arc::new(FihStorage::new(TestIo, "test-proj"))
}

#[test]
fn test_governance_submit_fact_with_gate() {
    let storage = make_storage();
    // Register schema via the gate
    let hash = storage.register_schema("text", b"text/plain");
    assert!(hash.is_some());

    let fact = Fact {
        id: FihHash::new(&["hello"], "fact"),
        origin: "text".into(),
        content: Content {
            mime_type: "text/plain".into(),
            data: b"hello world".to_vec(),
        },
        creator: "tester".into(),
    };

    let result = futures_executor::block_on(storage.submit_fact(&fact));
    assert!(result.is_ok());
    assert!(storage.evidence_tip().is_some());
}

#[test]
fn test_governance_submit_fact_rejected_by_gate() {
    let storage = make_storage();
    // No schema registered for "unknown"

    let fact = Fact {
        id: FihHash::new(&["data"], "fact"),
        origin: "unknown".into(),
        content: Content {
            mime_type: "text/plain".into(),
            data: b"some data".to_vec(),
        },
        creator: "tester".into(),
    };

    let result = futures_executor::block_on(storage.submit_fact(&fact));
    match result.unwrap_err() {
        nexus_model::BlackboardError::Forbidden(msg) => {
            assert!(msg.contains("not registered"), "msg: {msg}")
        }
        other => panic!("expected Forbidden error, got: {other:?}"),
    }
}

#[test]
fn test_governance_pass_through_when_disabled() {
    let storage = make_storage_disabled();

    let fact = Fact {
        id: FihHash::new(&["x"], "fact"),
        origin: "unknown".into(),
        content: Content {
            mime_type: "text/plain".into(),
            data: b"data".to_vec(),
        },
        creator: "tester".into(),
    };

    // Disabled governance passes through even without schema
    let result = futures_executor::block_on(storage.submit_fact(&fact));
    assert!(result.is_ok());
}

#[test]
fn test_no_governance_unconfigured() {
    let storage = make_storage_no_gov();

    // No governance field at all — should pass through
    assert!(!storage.governance_enabled());

    let fact = Fact {
        id: FihHash::new(&["y"], "fact"),
        origin: "any".into(),
        content: Content::from("data"),
        creator: "tester".into(),
    };

    let result = futures_executor::block_on(storage.submit_fact(&fact));
    assert!(result.is_ok());
}

#[test]
fn test_governance_toggle_runtime() {
    let storage = make_storage_disabled();
    assert!(!storage.governance_enabled());
    storage.set_governance(true);
    assert!(storage.governance_enabled());
    storage.set_governance(false);
    assert!(!storage.governance_enabled());
}

#[test]
fn test_governance_evidence_tip() {
    let storage = make_storage();
    storage.register_schema("num", b"i64");

    // Before submit: no evidence
    assert!(storage.evidence_tip().is_none());

    let fact = Fact {
        id: FihHash::new(&["42"], "fact"),
        origin: "num".into(),
        content: Content::from("42"),
        creator: "test".into(),
    };

    futures_executor::block_on(storage.submit_fact(&fact)).unwrap();
    // After submit: evidence recorded
    assert!(storage.evidence_tip().is_some());
    assert!(storage.verify_evidence(0));
}

#[test]
fn test_governance_hint_check() {
    let storage = make_storage();
    storage.with_hints(|h| {
        h.add("positive", nex::contract::HintRule::Positive);
    });

    assert!(storage.check_hints(5).is_ok());
    assert!(storage.check_hints(-1).is_err());
}

#[test]
fn test_governance_clear_hints() {
    let storage = make_storage();
    storage.with_hints(|h| {
        h.add("h1", nex::contract::HintRule::Gt(5));
        h.add("h2", nex::contract::HintRule::Lt(100));
    });

    storage.with_hints(|h| {
        assert_eq!(h.len(), 2);
        h.clear();
    });

    storage.with_hints(|h| {
        assert!(h.is_empty());
    });
}

#[test]
fn test_governance_manual_evidence() {
    let storage = make_storage();
    storage.record_evidence("custom-action", "custom-type");
    assert!(storage.evidence_tip().is_some());
}

#[test]
fn test_no_governance_evidence_is_empty() {
    let storage = make_storage_no_gov();
    // evidence_tip returns None when governance is not configured
    assert!(storage.evidence_tip().is_none());
    // verify_evidence returns true (no chain = no tamper)
    assert!(storage.verify_evidence(0));
}
