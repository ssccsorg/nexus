use crate::cold_query::ColdQuery;
use nexus_model::Content;
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
use std::collections::HashMap;

pub trait CypherCapable: nexus_model::StorageRead {
    /// Execute a compiled query plan against this storage backend.
    ///
    /// Returns a list of result rows. Each row maps field names to Content values.
    /// The Content mime_type indicates the format of each value (text/plain, application/json, etc.).
    fn query_plan(&self, _plan: &ColdQuery) -> Result<Vec<HashMap<String, Content>>, String> {
        Err("CypherCapable: not yet implemented for this backend".into())
    }
}

impl CypherCapable for nexus_model::NullStorage {}
