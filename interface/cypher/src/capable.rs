// Re-exports QueryCapable from interface-query.
// CypherCapable type alias preserved for backward compatibility.
pub use interface_query::QueryCapable;

/// Backward-compatible alias for `QueryCapable`.
pub use interface_query::QueryCapable as CypherCapable;
