/// Storage backend that can execute Cypher query plans.
///
/// `CypherCapable` marks a storage backend that accepts compiled Cypher
/// query plans (as `ColdQuery` structs) and returns results as a JSON
/// string. The primary implementation is `DuckDbStorage`, which translates
/// `ColdQuery` into DuckDB SQL via the `cypher_sql` translator.
///
/// `DualStorage` and `DefaultBlackboard` delegate to their cold backend.
/// Backends without a concrete implementation fall through to the default
/// error-returning method.
use crate::cold_query::ColdQuery;

pub trait CypherCapable: nexus_model::StorageRead {
    /// Execute a compiled query plan against this storage backend.
    ///
    /// The `plan` is a `ColdQuery` describing a tabular scan with filters,
    /// projections, ordering, and pagination. Returns a JSON string
    /// containing an array of result rows (each row is a JSON object).
    fn query_plan(&self, _plan: &ColdQuery) -> Result<String, String> {
        Err("CypherCapable: not yet implemented for this backend".into())
    }
}

impl CypherCapable for nexus_model::NullStorage {}
