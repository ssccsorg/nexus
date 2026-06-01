pub mod blackboard;
pub mod mock_gateway;
pub mod process;
pub mod query;
pub mod storage;

// Re-export key types for convenience
pub use blackboard::{DefaultBlackboard, Record};
pub use mock_gateway::MockGateway;
pub use nexus_model::{
    Blackboard, BlackboardError, BoardState, Content, Fact, FihHash, Hint, Intent,
};
pub use process::{
    error::ProcessError,
    scheduler::Scheduler,
};
pub use query::cypher::capable::CypherCapable;
pub use storage::petgraph::{
    GraphRead, GraphWrite, NodeWeight, EdgeWeight, PetgraphStorage, Snapshottable, StorageSnapshot,
};
pub use storage::composite::CompositeColdStorage;

/// Create a default blackboard with in-memory petgraph hot storage
/// and no cold backend.
pub fn create_blackboard() -> DefaultBlackboard {
    DefaultBlackboard::new()
}
