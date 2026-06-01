// Shared application state for the gateway API server.

use nexus::{Blackboard, create_blackboard};
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
            blackboard: Arc::new(Mutex::new(Box::new(create_blackboard()))),
        }
    }
}
