// nexus-graph — Query frontends.
//
// The query layer translates graph query languages into execution plans
// against petgraph (hot) and DuckDB/SQL (cold) backends.
//
// Current: Cypher
// Future: GQL, SQL/PGQ

pub mod cypher;
