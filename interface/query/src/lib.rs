/// Common tabular query types for cold storage routing.
///
/// These types describe a tabular query against cold storage (Parquet-backed
/// DuckDB views, or any other tabular store). Query frontends (Cypher, GQL,
/// SQL/PGQ) produce these when they determine a query should be routed to
/// cold storage instead of the hot graph.
///
/// `serde_json::Value` is retained for filter literal values because cold
/// backends accept various literal types (strings, numbers, booleans).
use nexus_model::Content;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// A tabular query for cold storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColdQuery {
    /// Target label: "Fact", "Intent", or "Hint".
    pub label: String,

    /// Column filters (AND semantics).
    #[serde(default)]
    pub filters: Vec<ColdFilter>,

    /// Output columns. Empty = select all columns.
    #[serde(default)]
    pub projections: Vec<String>,

    /// Sort specifications.
    #[serde(default)]
    pub order_by: Vec<ColdOrder>,

    /// Maximum rows to return.
    pub limit: Option<usize>,

    /// Row offset.
    pub offset: Option<usize>,

    /// True to emit SELECT DISTINCT.
    #[serde(default)]
    pub distinct: bool,

    /// If true, emit COUNT(*) instead of column projection.
    #[serde(default)]
    pub aggregate_count: bool,

    /// GROUP BY columns.
    #[serde(default)]
    pub group_by: Vec<String>,

    /// Aggregate function projections (SUM, AVG, MIN, MAX, COUNT, COUNT_DISTINCT).
    #[serde(default)]
    pub aggregates: Vec<AggregateDef>,
}

impl ColdQuery {
    /// Create a new ColdQuery with default values for all optional fields.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            filters: Vec::new(),
            projections: Vec::new(),
            order_by: Vec::new(),
            limit: None,
            offset: None,
            distinct: false,
            aggregate_count: false,
            group_by: Vec::new(),
            aggregates: Vec::new(),
        }
    }
}

/// A single filter condition (AND-composed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColdFilter {
    /// Column name (e.g. "origin", "creator", "fact_id").
    pub field: String,

    /// Operator: "Eq", "Ne", "Gt", "Lt", "Gte", "Lte", "In", "Contains",
    /// "FtsMatch", "FtsMatchAnd", "FtsMatchOr".
    pub op: String,

    /// Operand value.
    pub value: Value,
}

/// Sort specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColdOrder {
    pub field: String,
    #[serde(default)]
    pub desc: bool,
}

/// Aggregate function definition for GROUP BY queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateDef {
    /// Function name: "SUM", "AVG", "MIN", "MAX", "COUNT", "COUNT_DISTINCT".
    pub func: String,
    /// Target column or expression.
    pub column: String,
    /// Output alias; defaults to "{func}({column})" if None.
    pub alias: Option<String>,
}

// ── QueryCapable trait ──────────────────────────────────────────────────

/// Storage backend that can execute tabular query plans.
///
/// Backends that can answer tabular queries (filter, project, aggregate)
/// implement this trait. The primary implementation is `DuckDbStorage`,
/// which translates `ColdQuery` into DuckDB SQL.
///
/// Backends without a concrete implementation fall through to the default
/// error-returning method.
pub trait QueryCapable: nexus_model::StorageRead {
    /// Execute a compiled query plan against this storage backend.
    ///
    /// Returns a list of result rows. Each row maps field names to Content
    /// values. The Content mime_type indicates the format of each value.
    fn query_plan(&self, _plan: &ColdQuery) -> Result<Vec<HashMap<String, Content>>, String> {
        Err("QueryCapable: not yet implemented for this backend".into())
    }
}

impl QueryCapable for nexus_model::NullStorage {}
