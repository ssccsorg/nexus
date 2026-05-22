use super::read::StorageRead;

/// Storage backend that can execute Cypher query plans.
///
/// `CypherCapable` marks a storage backend that accepts compiled Cypher
/// query plans (serialized as `ColdQuery` JSON) and returns results as
/// a JSON array. The primary implementation is `DuckDbStorage`, which
/// translates `ColdQuery` into DuckDB SQL via the `cypher_sql` translator.
///
/// `DualStorage` and `DefaultBlackboard` delegate to their cold backend.
/// Backends without a concrete implementation fall through to the default
/// error-returning method.
pub trait CypherCapable: StorageRead {
    /// Execute a compiled query plan against this storage backend.
    ///
    /// The `plan` is a JSON-serialized `ColdQuery` describing a tabular
    /// scan with filters, projections, ordering, and pagination.
    /// Returns a JSON array of result rows (each row is a JSON object).
    fn query_plan(&self, _plan: &serde_json::Value) -> Result<serde_json::Value, String> {
        Err("CypherCapable: not yet implemented for this backend".into())
    }
}
