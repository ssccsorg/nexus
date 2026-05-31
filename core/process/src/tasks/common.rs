// Common helpers shared across detection task implementations.

use nexus_model::Fact;

/// Extract the `topic` field from a Fact's JSON content.
/// Returns `None` if the field is missing or not a string.
use serde_json::Value;

/// Extract the `topic` field from a Fact's JSON content.
/// Returns `None` if the field is missing or not a string.
pub(crate) fn topic_of(fact: &Fact) -> Option<String> {
    let v: Value = serde_json::from_str(fact.content.as_str()?).unwrap_or(Value::Null);
    v.get("topic")?.as_str().map(|s| s.to_string())
}

/// Extract the `position` field from a Fact's JSON content.
/// Returns `None` if the field is missing or not a string.
pub(crate) fn position_of(fact: &Fact) -> Option<String> {
    let v: Value = serde_json::from_str(fact.content.as_str()?).unwrap_or(Value::Null);
    v.get("position")?.as_str().map(|s| s.to_string())
}
