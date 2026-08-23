//! Unit tests for nexd process supervision logic.
//!
//! The respawn circuit breaker is extracted as a pure decision function
//! so the crash-loop policy is testable without spawning processes.

use nexd::manager::respawn_decision;

#[test]
fn test_respawn_decision_rapid_exits_advance() {
    let mut failures = 0;
    for expected in 1..=4 {
        let (f, should) = respawn_decision(failures, true);
        assert_eq!(f, expected, "rapid exit increments the counter");
        assert!(should, "below the threshold respawning continues");
        failures = f;
    }
    let (f, should) = respawn_decision(failures, true);
    assert_eq!(f, 5, "fifth rapid exit reaches the threshold");
    assert!(!should, "at the threshold respawning stops");
}

#[test]
fn test_respawn_decision_survival_resets() {
    // A child that survives the rapid-exit window resets the counter and
    // keeps respawning allowed for a later crash.
    let (f, should) = respawn_decision(4, false);
    assert_eq!(f, 0, "a survived window resets the counter");
    assert!(should, "a reset counter allows respawn");
}

#[test]
fn test_respawn_decision_past_threshold_stays_stopped() {
    let (f, should) = respawn_decision(5, true);
    assert_eq!(f, 6);
    assert!(!should, "beyond the threshold respawning stays stopped");
}
