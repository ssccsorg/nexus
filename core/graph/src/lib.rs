// nexus-graph — Graph runtime: Cypher query frontend + Blackboard + dispatcher.
//
// Architecture
// ============
//
//   nexus-graph (graph runtime)
//     ├── query/cypher/     ← Cypher query frontend (future: GQL, SQL/PGQ)
//     ├── blackboard/       ← DefaultBlackboard: storage coordinator
//     └── dispatcher/       ← OODA loop / stigmergy runtime (future, #35)

pub mod blackboard;
pub mod mock_gateway;
pub mod query;

// Re-export cypher at top level for backward compat with tests and playbooks.
// Full path: nexus_graph::query::cypher or nexus_graph::cypher.
pub use query::cypher;

// DefaultBlackboard is crate-internal. External consumers use the factory.
use blackboard::DefaultBlackboard;

/// Create a default blackboard with in-memory petgraph hot storage
/// and no cold backend. Equivalent to `DefaultBlackboard::new()`.
pub fn create_blackboard()
-> impl Blackboard + CypherCapable + EvictCapable + FlushCapable + GraphRead + Snapshottable {
    DefaultBlackboard::new()
}

/// Create a blackboard with custom hot and cold storage.
pub fn create_blackboard_with_storage(
    hot: PetgraphStorage,
    cold: Box<dyn ColdStorage>,
) -> impl Blackboard + CypherCapable + EvictCapable + FlushCapable + GraphRead + Snapshottable {
    DefaultBlackboard::with_storage(hot, cold)
}

/// Reconstruct a blackboard from a previously saved snapshot.
/// Equivalent to `DefaultBlackboard::from_snapshot()`.
pub fn create_blackboard_from_snapshot(
    snapshot: StorageSnapshot,
) -> impl Blackboard + CypherCapable + EvictCapable + FlushCapable + GraphRead + Snapshottable {
    DefaultBlackboard::from_snapshot(snapshot)
}

/// Reconstruct a blackboard from a snapshot with a custom cold storage.
/// Use this after worker restart: the snapshot restores the hot petgraph
/// graph, flush cursor, and claims, while the caller provides the cold
/// backend (e.g., a fresh CompositeColdStorage with current KV bindings).
pub fn create_blackboard_from_snapshot_with_cold(
    snapshot: StorageSnapshot,
    cold: Box<dyn ColdStorage>,
) -> impl Blackboard + CypherCapable + EvictCapable + FlushCapable + GraphRead + Snapshottable {
    DefaultBlackboard::from_snapshot_with_cold(&snapshot, cold)
}

pub use nexus_model::{
    Blackboard, BlackboardError, BoardState, ColdStorage, CypherCapable, DualStorage, EvictCapable,
    Fact, FactCapable, FihHash, FihPersistence, FilterCapable, FlushCapable, Hint, HintCapable,
    HotStorage, Intent, IntentCapable, NullStorage, ScanCapable, StateFilter, StorageRead,
    TimeRangeCapable,
};
pub use nexus_storage_petgraph::{
    EdgeWeight, GraphRead, GraphWrite, NodeWeight, PetgraphStorage, Record, Snapshottable,
    StorageSnapshot,
};
