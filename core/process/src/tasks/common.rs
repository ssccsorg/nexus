// Common helpers shared across detection task implementations.

use nexus_model::Fact;

/// Extract the `topic` field from a Fact's JSON content.
/// Returns `None` if the field is missing or not a string.
pub(crate) fn topic_of(fact: &Fact) -> Option<&str> {
    fact.content.get("topic")?.as_str()
}

/// Extract the `position` field from a Fact's JSON content.
/// Returns `None` if the field is missing or not a string.
pub(crate) fn position_of(fact: &Fact) -> Option<&str> {
    fact.content.get("position")?.as_str()
}
