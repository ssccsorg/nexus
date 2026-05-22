// Shared application state for the gateway API server.

use nexus_graph::{create_blackboard, create_blackboard_with_storage, Blackboard};
use nexus_storage_petgraph::PetgraphStorage;
use nexus_storage_sqlite::SqlNormalizedStorage;
use std::sync::{Arc, Mutex};

/// Thread-safe shared state wrapping a Blackboard.
///
/// The inner Mutex provides the same synchronization pattern used by
/// the parallel stress tests in core/graph/tests/.
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

    /// Create state backed by SQLite at the given path.
    pub fn with_sqlite(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let project_id = "default";
        let cold = SqlNormalizedStorage::open_with_project(path, project_id)?;
        let hot = PetgraphStorage::with_project_id(project_id);
        let bb = create_blackboard_with_storage(hot, Box::new(cold));
        Ok(Self {
            blackboard: Arc::new(Mutex::new(Box::new(bb))),
        })
    }
}
