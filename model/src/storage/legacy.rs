use serde::{Deserialize, Serialize};

/// Legacy event-log types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEvent {
    pub event_type: String,
    pub payload: String,
}
