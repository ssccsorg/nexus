// The detection capability's defaults: a detector that does not persist state is
// stateless, and a restore into it changes nothing.
//
// `DetectionCapable` provides both defaults so a detector implements only what it
// provides. The process layer's state snapshots rely on that: a detector with nothing to
// say about its state must answer nothing rather than an empty state that a reader would
// mistake for a checkpoint.

use nex_fih::{BoardState, Content, DetectionCapable, DetectionOutput};

struct Stateless;

impl DetectionCapable for Stateless {
    fn name(&self) -> &str {
        "stateless"
    }

    fn orient(&mut self, _state: &BoardState) -> DetectionOutput {
        DetectionOutput::default()
    }
}

fn empty_state() -> BoardState {
    BoardState {
        facts: Vec::new(),
        intents: Vec::new(),
        hints: Vec::new(),
    }
}

#[test]
fn a_detector_that_does_not_persist_state_answers_no_snapshot() {
    let mut detector = Stateless;

    assert_eq!(detector.name(), "stateless");
    assert!(detector.snapshot_state().is_none());
    assert!(detector.orient(&empty_state()).facts.is_empty());

    detector.restore_state(Content::from("ignored"));
    assert!(
        detector.snapshot_state().is_none(),
        "restoring into a stateless detector does not give it a state"
    );
}
