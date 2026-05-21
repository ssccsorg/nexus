use super::read::StorageRead;

/// Placeholder trait for future Cypher-to-storage translation.
///
/// This trait marks a storage backend that can execute Cypher query plans
/// directly. SSCCS-Nexus will implement a `CypherCapable` adapter for
/// `DuckDbStorage` that translates `cyrs_plan::ReadOp` chains into DuckDB
/// SQL with `read_parquet()` (#51).
///
/// Until then, this trait serves as a type-system placeholder confirming
/// that the architecture intends for storage backends to be reachable
/// through Cypher queries, not just through the primitive FIH CRUD traits.
pub trait CypherCapable: StorageRead {
    /// Execute a compiled query plan against this storage backend.
    ///
    /// Currently unimplemented. Defined now to reserve the interface slot.
    fn query_plan(&self, _plan: &serde_json::Value) -> Result<serde_json::Value, String> {
        Err("CypherCapable: not yet implemented for this backend".into())
    }
}

// Concrete backends implement `CypherCapable` explicitly. Backends that
// do not implement it fall through to the trait's default error-returning
// method.
//
// There is intentionally no blanket impl: Rust does not allow overriding
// blanket impls with concrete impls, so each backend must be registered
// explicitly.
