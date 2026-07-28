// ── GovernanceGate tests ──────────────────────────────────────────────

use nex::contract::{GovernanceBypassError, GovernanceGate};

#[test]
fn test_gate_new_is_empty() {
    let gate = GovernanceGate::new();
    assert_eq!(gate.schema_count(), 0);
}

#[test]
fn test_register_and_admit() {
    let gate = GovernanceGate::new();
    let hash = gate.register_schema("number", b"i64");
    assert_eq!(hash.len(), 64);
    assert!(gate.has_schema("number"));
    assert!(gate.admit("number", b"42").is_ok());
}

#[test]
fn test_admit_unknown_schema_fails() {
    let gate = GovernanceGate::new();
    let err = gate.admit("unknown", b"data").unwrap_err();
    assert!(err.reason.contains("not registered"));
}

#[test]
fn test_verify_passes_on_match() {
    let gate = GovernanceGate::new();
    gate.register_schema("test", b"hello");
    assert!(gate.verify("test", b"hello").is_ok());
}

#[test]
fn test_verify_fails_on_drift() {
    let gate = GovernanceGate::new();
    gate.register_schema("test", b"hello");
    let err = gate.verify("test", b"world").unwrap_err();
    assert!(err.reason.contains("hash mismatch"));
}

#[test]
fn test_verify_unregistered_passes() {
    let gate = GovernanceGate::new();
    assert!(gate.verify("nonexistent", b"data").is_ok());
}

#[test]
fn test_unregister() {
    let gate = GovernanceGate::new();
    gate.register_schema("a", b"schema_a");
    assert!(gate.has_schema("a"));
    gate.unregister_schema("a");
    assert!(!gate.has_schema("a"));
}

#[test]
fn test_clear() {
    let gate = GovernanceGate::new();
    gate.register_schema("a", b"aaa");
    gate.register_schema("b", b"bbb");
    assert_eq!(gate.schema_count(), 2);
    gate.clear();
    assert_eq!(gate.schema_count(), 0);
}

#[test]
fn test_register_duplicate_overwrites() {
    let gate = GovernanceGate::new();
    let h1 = gate.register_schema("x", b"content1");
    let h2 = gate.register_schema("x", b"content2");
    assert_ne!(h1, h2);
    assert!(gate.has_schema("x"));
    assert!(gate.verify("x", b"content2").is_ok());
}

#[test]
fn test_gate_error_display() {
    let err = GovernanceBypassError {
        reason: "test reason".into(),
    };
    let msg = err.to_string();
    assert!(msg.contains("test reason"));
}
