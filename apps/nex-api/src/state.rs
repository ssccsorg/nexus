// Shared application state for the gateway API server.

use nexus_storage_composite::HybridBlackboard;
use nexus_model::Blackboard;
use std::sync::{Arc, Mutex};

/// Thread-safe shared state wrapping a Blackboard.
#[derive(Clone)]
pub struct AppState {
    pub blackboard: Arc<Mutex<Box<dyn Blackboard + Send>>>,
}

impl AppState {
    /// Create in-memory state (no persistence).
    pub fn in_memory() -> Self {
        Self {
            blackboard: Arc::new(Mutex::new(Box::new(HybridBlackboard::new()))),
        }
    }
}
