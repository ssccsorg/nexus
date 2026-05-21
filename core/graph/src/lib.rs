// nexus-graph — GraphBlackboard: petgraph-backed FIH Blackboard.
//
// Architecture
// ============
//
//   GraphBlackboard (implements Blackboard + GraphAccess)
//     ├── storage: DualStorage (hot + cold)
//     │     ├── hot: PetgraphStorage (Arc<Mutex<petgraph::Graph>>)
//     │     └── cold: ColdStorage (NullStorage | any external impl)
//     ├── hot_graph: Arc<Mutex<petgraph::Graph>> (shared with PetgraphStorage)
//     ├── claims: Mutex<HashMap<IntentId, Agent>>
//     └── project_id: String

pub mod cypher;
pub mod graph_access;
pub mod graph_blackboard;
pub mod mock_gateway;
pub mod petgraph_storage;
pub mod weight;

pub use graph_access::GraphAccess;
pub use graph_blackboard::GraphBlackboard;
pub use nexus_model::{
    Blackboard, BlackboardError, BoardState, ColdStorage, CypherCapable, DualStorage, EvictCapable,
    Fact, FactCapable, FihHash, FihPersistence, FilterCapable, FlushCapable, Hint, HintCapable,
    HotStorage, Intent, IntentCapable, NullStorage, ScanCapable, StateFilter, StorageRead,
    TimeRangeCapable,
};
pub use petgraph_storage::PetgraphStorage;
pub use weight::{EdgeWeight, NodeWeight, Record};
