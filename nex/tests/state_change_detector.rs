use nex::helper::ContentJsonExt;
use nex::process::tasks::state_change_detector::StateChangeDetector;
use nexus_model::{BoardState, Content, DetectionCapable, Fact, FihHash};

fn make_fact(id: &str, origin: &str) -> Fact {
    Fact {
        id: FihHash(id.to_string()),
        origin: origin.to_string(),
        content: Content::from_json(&serde_json::json!({"topic": "test"})),
        creator: "test".into(),
    }
}

#[test]
fn first_call_silent() {
    let mut d = StateChangeDetector::new();
    let s = BoardState {
        facts: vec![make_fact("f1", "a")],
        intents: vec![],
        hints: vec![],
    };
    let o = d.orient(&s);
    assert!(o.facts.is_empty());
}

#[test]
fn detects_fact_increase() {
    let mut d = StateChangeDetector::new();
    d.orient(&BoardState {
        facts: vec![make_fact("f1", "a")],
        intents: vec![],
        hints: vec![],
    });
    let o = d.orient(&BoardState {
        facts: vec![make_fact("f1", "a"), make_fact("f2", "b")],
        intents: vec![],
        hints: vec![],
    });
    assert_eq!(o.facts.len(), 1);
    assert!(
        o.facts[0]
            .content
            .try_parse_json::<serde_json::Value>()
            .unwrap_or(serde_json::Value::Null)["type"]
            .as_str()
            == Some("state_change")
    );
}

#[test]
fn no_change_no_fact() {
    let mut d = StateChangeDetector::new();
    let s = BoardState {
        facts: vec![make_fact("f1", "a")],
        intents: vec![],
        hints: vec![],
    };
    d.orient(&s);
    let o = d.orient(&s);
    assert!(o.facts.is_empty());
}
