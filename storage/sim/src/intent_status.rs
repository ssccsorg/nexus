// ── intent_status: Intent lifecycle transitions ─────────────────────────
//
// IntentStatus enum with compile-time enforcement of valid transitions.
// Invalid transitions return Err without modifying state.

pub use crate::record::IntentStatus;

impl IntentStatus {
    /// Attempt to transition from the current state to Claimed.
    /// Returns an error if already Claimed or Concluded.
    pub fn try_claim(&self, worker: &str, heartbeat_at: u64) -> Result<IntentStatus, String> {
        match self {
            IntentStatus::Submitted => Ok(IntentStatus::Claimed {
                worker: worker.to_string(),
                last_heartbeat_at: heartbeat_at,
            }),
            IntentStatus::Claimed { worker: w, .. } => Err(format!("already claimed by {}", w)),
            IntentStatus::Concluded { .. } => Err("already concluded".to_string()),
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
