// ── FIH-specific contract tests ───────────────────────────────────────

use nex::contract::{core::GovernanceGate, fih};

#[test]
fn test_register_default_fih_schemas() {
    let gate = GovernanceGate::new();
    fih::register_default_fih_schemas(&gate);

    assert_eq!(gate.schema_count(), 3);
    assert!(gate.has_schema("number"));
    assert!(gate.has_schema("text"));
    assert!(gate.has_schema("blob"));

    // Verify schemas are admitted correctly
    assert!(gate.admit("number", b"42").is_ok());
    assert!(gate.admit("text", b"hello").is_ok());
    assert!(gate.admit("blob", b"\x00\x01\x02").is_ok());
}

#[test]
fn test_constraint_factories() {
    use nex::contract::fih::constraints;

    let pos = constraints::positive();
    assert!(pos.check_numeric(1));
    assert!(!pos.check_numeric(0));

    let even = constraints::even();
    assert!(even.check_numeric(42));
    assert!(!even.check_numeric(7));

    let gt5 = constraints::gt(5);
    assert!(gt5.check_numeric(6));
    assert!(!gt5.check_numeric(5));

    let lt10 = constraints::lt(10);
    assert!(lt10.check_numeric(9));
    assert!(!lt10.check_numeric(10));

    let non_neg = constraints::non_negative();
    assert!(non_neg.check_numeric(0));
    assert!(non_neg.check_numeric(42));
    assert!(!non_neg.check_numeric(-1));

    let eq42 = constraints::eq(42);
    assert!(eq42.check_numeric(42));
    assert!(!eq42.check_numeric(41));
}
