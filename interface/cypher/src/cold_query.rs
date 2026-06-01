// Cold query types for cold storage routing.
//
// These types describe a tabular query against cold storage (Parquet-backed
// DuckDB views). The Cypher executor produces these when it determines a
// query should be routed to cold storage instead of the hot petgraph.
//
// The canonical definition lives in `storage/duckdb/src/cold_query.rs`.
// This module re-exports for convenience of the interface layer.

// Re-export from storage/duckdb if available, or define locally.
// For now, the struct is in `storage/duckdb::cold_query::ColdQuery`
// and used by plan.rs via serde_json::Value serialization.
