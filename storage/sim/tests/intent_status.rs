// IntentStatus state machine tests.
// Tests: submit→claim→conclude lifecycle, double claim rejection,
// wrong worker heartbeat rejection, concluded is not active, conclude unclaimed.

use nexus_storage_sim::intent_status::IntentStatus;

#[test]
fn test_submit_to_claim_to_conclude() {
    let s = IntentStatus::Submitted;
    let claimed = s.try_claim("alice", 100).unwrap();
    assert!(matches!(claimed, IntentStatus::Claimed { ref worker, .. } if worker == "alice"));

    let heartbeat = claimed.try_heartbeat("alice", 200).unwrap();
    assert!(
        matches!(heartbeat, IntentStatus::Claimed { last_heartbeat_at, .. } if last_heartbeat_at == 200)
    );

    let concluded = claimed.try_conclude("f_result", 300).unwrap();
    assert!(matches!(concluded, IntentStatus::Concluded { to_fact, .. } if to_fact == "f_result"));
}

#[test]
fn test_double_claim_rejected() {
    let s = IntentStatus::Submitted;
    let claimed = s.try_claim("alice", 100).unwrap();
    assert!(claimed.try_claim("bob", 200).is_err());
}

#[test]
fn test_wrong_worker_heartbeat_rejected() {
    let s = IntentStatus::Submitted;
    let claimed = s.try_claim("alice", 100).unwrap();
    assert!(claimed.try_heartbeat("bob", 200).is_err());
}

#[test]
fn test_concluded_is_not_active() {
    let s = IntentStatus::Submitted;
    let claimed = s.try_claim("alice", 100).unwrap();
    let concluded = claimed.try_conclude("f_result", 300).unwrap();
    assert!(!concluded.is_active());
}

#[test]
fn test_conclude_unclaimed_rejected() {
    let s = IntentStatus::Submitted;
    assert!(s.try_conclude("f_result", 100).is_err());
}
