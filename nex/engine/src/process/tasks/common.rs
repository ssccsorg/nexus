// Common helpers shared across detection task implementations.

use crate::helper::ContentJsonExt;
use nexus_model::Fact;

/// Extract the `topic` field from a Fact's JSON content.
/// Returns `None` if the field is missing or not a string.
pub(crate) fn topic_of(fact: &Fact) -> Option<String> {
    let v: serde_json::Value = fact.content.try_parse_json()?;
    v.get("topic")?.as_str().map(|s| s.to_string())
}

/// Extract the `position` field from a Fact's JSON content.
/// Returns `None` if the field is missing or not a string.
pub(crate) fn position_of(fact: &Fact) -> Option<String> {
    let v: serde_json::Value = fact.content.try_parse_json()?;
    v.get("position")?.as_str().map(|s| s.to_string())
}
