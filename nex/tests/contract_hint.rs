// ── HintEngine tests ──────────────────────────────────────────────────

use nex::contract::{HintEngine, HintRule};

#[test]
fn test_hint_engine_new_is_empty() {
    let engine = HintEngine::new();
    assert!(engine.is_empty());
    assert_eq!(engine.len(), 0);
}

#[test]
fn test_add_and_check() {
    let engine = HintEngine::new();
    engine.add("h1", HintRule::Gt(10));
    assert!(engine.check_numeric(20).is_ok());
    assert!(engine.check_numeric(5).is_err());
}

#[test]
fn test_multiple_hints() {
    let engine = HintEngine::new();
    engine.add("pos", HintRule::Positive);
    engine.add("even", HintRule::Even);
    assert!(engine.check_numeric(42).is_ok());
    assert!(engine.check_numeric(-4).is_err());
    assert!(engine.check_numeric(7).is_err());
}

#[test]
fn test_remove() {
    let engine = HintEngine::new();
    engine.add("h1", HintRule::Gt(10));
    engine.add("h2", HintRule::Lt(100));
    assert_eq!(engine.len(), 2);
    engine.remove("h1");
    assert_eq!(engine.len(), 1);
    assert!(engine.check_numeric(5).is_ok());
}

#[test]
fn test_clear() {
    let engine = HintEngine::new();
    engine.add("a", HintRule::Even);
    engine.add("b", HintRule::Positive);
    engine.clear();
    assert!(engine.is_empty());
    assert!(engine.check_numeric(-3).is_ok());
}

#[test]
fn test_parse_rules() {
    assert_eq!(HintRule::parse("gt 10"), Some(HintRule::Gt(10)));
    assert_eq!(HintRule::parse("lt -5"), Some(HintRule::Lt(-5)));
    assert_eq!(HintRule::parse("eq 42"), Some(HintRule::Eq(42)));
    assert_eq!(HintRule::parse("ne 0"), Some(HintRule::Ne(0)));
    assert_eq!(HintRule::parse("positive"), Some(HintRule::Positive));
    assert_eq!(HintRule::parse("even"), Some(HintRule::Even));
    assert!(HintRule::parse("gt").is_none());
    assert!(HintRule::parse("unknown").is_none());
}

#[test]
fn test_check_numeric_edge_cases() {
    assert!(HintRule::Gt(10).check_numeric(11));
    assert!(!HintRule::Gt(10).check_numeric(10));
    assert!(HintRule::Lt(10).check_numeric(9));
    assert!(!HintRule::Lt(10).check_numeric(10));
    assert!(HintRule::Eq(5).check_numeric(5));
    assert!(!HintRule::Eq(5).check_numeric(4));
    assert!(HintRule::Ne(5).check_numeric(4));
    assert!(!HintRule::Ne(5).check_numeric(5));
    assert!(HintRule::Positive.check_numeric(1));
    assert!(!HintRule::Positive.check_numeric(0));
    assert!(!HintRule::Positive.check_numeric(-1));
    assert!(HintRule::Even.check_numeric(0));
    assert!(HintRule::Even.check_numeric(42));
    assert!(!HintRule::Even.check_numeric(7));
}

#[test]
fn test_schema_hints_always_pass_in_v1() {
    assert!(HintRule::FromSchema("test".into()).check_numeric(0));
    assert!(HintRule::ToSchema("test".into()).check_numeric(0));
    assert!(HintRule::Custom("any".into()).check_numeric(0));
}

#[test]
fn test_rule_describe() {
    assert_eq!(HintRule::Gt(10).describe(), "value > 10");
    assert_eq!(HintRule::Positive.describe(), "value > 0");
    assert!(HintRule::FromSchema("s".into()).describe().contains("schema"));
}

#[test]
fn test_engine_all_and_get() {
    let engine = HintEngine::new();
    engine.add("a", HintRule::Gt(5));
    engine.add("b", HintRule::Even);
    assert_eq!(engine.all().len(), 2);
    assert!(engine.get("a").is_some());
    assert!(engine.get("nonexistent").is_none());
}
