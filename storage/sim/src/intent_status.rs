// ── intent_status: Intent lifecycle transitions ─────────────────────────
//
// IntentStatus enum with compile-time enforcement of valid transitions.
// Invalid transitions return Err without modifying state.

use crate::record::IntentStatus;

impl IntentStatus {
    /// Attempt to transition from the current state to Claimed.
    /// Returns an error if already Claimed or Concluded.
    pub fn try_claim(&self, worker: &str, heartbeat_at: u64) -> Result<IntentStatus, String> {
        match self {
            IntentStatus::Submitted => Ok(IntentStatus::Claimed {
                worker: worker.to_string(),
                last_heartbeat_at: heartbeat_at,
            }),
            IntentStatus::Claimed { worker: w, .. } => {
                Err(format!("already claimed by {}", w))
            }
            IntentStatus::Concluded { .. } => {
                Err("already concluded".to_string())
            }
        }
    }

    /// Attempt to transition to Claimed with updated heartbeat.
    /// Only valid if currently Claimed by the same worker.
    pub fn try_heartbeat(&self, agent: &str, heartbeat_at: u64) -> Result<IntentStatus, String> {
        match self {
            IntentStatus::Claimed {
                worker,
                last_heartbeat_at: _,
            } if worker == agent => Ok(IntentStatus::Claimed {
                worker: worker.clone(),
                last_heartbeat_at: heartbeat_at,
            }),
            IntentStatus::Claimed { worker, .. } => {
                Err(format!("claimed by {}, not {}", worker, agent))
            }
            IntentStatus::Submitted => Err("not claimed".to_string()),
            IntentStatus::Concluded { .. } => Err("already concluded".to_string()),
        }
    }

    /// Attempt to transition from Claimed to Concluded.
    pub fn try_conclude(&self, to_fact: &str, concluded_at: u64) -> Result<IntentStatus, String> {
        match self {
            IntentStatus::Claimed { worker, .. } => Ok(IntentStatus::Concluded {
                to_fact: to_fact.to_string(),
                concluded_at,
                worker: worker.clone(),
            }),
            IntentStatus::Submitted => Err("not claimed".to_string()),
            IntentStatus::Concluded { .. } => Err("already concluded".to_string()),
        }
    }

    /// Check if the intent is still active (not concluded).
    pub fn is_active(&self) -> bool {
        matches!(self, IntentStatus::Submitted | IntentStatus::Claimed { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_submit_to_claim_to_conclude() {
        let s = IntentStatus::Submitted;
        let claimed = s.try_claim("alice", 100).unwrap();
        assert!(matches!(claimed, IntentStatus::Claimed { ref worker, .. } if worker == "alice"));

        let heartbeat = claimed.try_heartbeat("alice", 200).unwrap();
        assert!(matches!(heartbeat, IntentStatus::Claimed { last_heartbeat_at, .. } if last_heartbeat_at == 200));

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
}
