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

pub use blackboard::DefaultBlackboard;
pub use nexus_model::{
    Blackboard, BlackboardError, BoardState, ColdStorage, CypherCapable, DualStorage, EvictCapable,
    Fact, FactCapable, FihHash, FihPersistence, FilterCapable, FlushCapable, Hint, HintCapable,
    HotStorage, Intent, IntentCapable, NullStorage, ScanCapable, StateFilter, StorageRead,
    TimeRangeCapable,
};
pub use nexus_storage_petgraph::{
    EdgeWeight, GraphRead, GraphWrite, NodeWeight, PetgraphStorage, Record,
};
