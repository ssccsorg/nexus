// DuckDB-specific query types moved to storage/duckdb/src/duckdb_ext.rs.
// Common types (ColdQuery, ColdFilter, ColdOrder, AggregateDef) moved to
// interface/query/src/lib.rs.
//
// This module is retained as a thin re-export for backward compatibility.
pub use interface_query::{AggregateDef, ColdFilter, ColdOrder, ColdQuery};
