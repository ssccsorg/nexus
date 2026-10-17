// The process error surface: one category per failure origin, and the conversion that
// keeps a blackboard failure's message intact.
//
// The process layer wraps the storage errors a caller sees. A category that renders the
// wrong word, or a conversion that drops the message, hides which layer failed.

use nex::ProcessError;
use nex_fih::BlackboardError;

#[test]
fn every_variant_renders_its_category_and_its_message() {
    let cases = [
        (
            ProcessError::Blackboard("no such fact".into()),
            "blackboard error: no such fact",
        ),
        (
            ProcessError::Eviction("the volume is full".into()),
            "eviction error: the volume is full",
        ),
        (
            ProcessError::Snapshot("bad postcard".into()),
            "snapshot error: bad postcard",
        ),
        (
            ProcessError::Internal("unreachable".into()),
            "internal error: unreachable",
        ),
    ];

    for (error, rendered) in cases {
        assert_eq!(error.to_string(), rendered);
    }
}

#[test]
fn a_blackboard_error_becomes_a_blackboard_category_with_its_message() {
    let error: ProcessError = BlackboardError::Conflict("two contents at one id".into()).into();

    match &error {
        ProcessError::Blackboard(message) => {
            assert_eq!(message, "conflict: two contents at one id");
        }
        other => panic!("a blackboard failure landed in another category: {other:?}"),
    }
    assert_eq!(
        error.to_string(),
        "blackboard error: conflict: two contents at one id"
    );
}
