/// Cold query types for cold storage routing.
///
/// These types describe a tabular query against cold storage (Parquet-backed
/// DuckDB views). The Cypher executor produces these when it determines a
/// query should be routed to cold storage instead of the hot petgraph.
///
/// `ColdQuery` is the query specification format. `serde_json::Value` is
/// retained for filter literal values because DuckDB accepts various literal
/// types (strings, numbers, booleans).
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A tabular query for cold storage (DuckDB).
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

    /// Common Table Expressions (WITH clause).
    #[serde(default)]
    pub with_ctes: Vec<CteDef>,

    /// GROUP BY columns.
    #[serde(default)]
    pub group_by: Vec<String>,

    /// Aggregate function projections (SUM, AVG, MIN, MAX, COUNT, COUNT_DISTINCT).
    #[serde(default)]
    pub aggregates: Vec<AggregateDef>,

    /// Window function projections.
    #[serde(default)]
    pub window_funcs: Vec<WindowFuncDef>,

    /// JSON property extraction columns.
    #[serde(default)]
    pub json_projections: Vec<JsonProjection>,

    /// JSON property path filters (AND with main filters).
    #[serde(default)]
    pub json_filters: Vec<JsonFilter>,

    /// Vector similarity threshold filters (AND).
    #[serde(default)]
    pub vector_filters: Vec<VectorFilter>,

    /// Vector similarity score projection (adds a score column + orders by it).
    pub vector_score: Option<VectorScore>,
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
            with_ctes: Vec::new(),
            group_by: Vec::new(),
            aggregates: Vec::new(),
            window_funcs: Vec::new(),
            json_projections: Vec::new(),
            json_filters: Vec::new(),
            vector_filters: Vec::new(),
            vector_score: None,
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

/// Common Table Expression: WITH alias AS (subquery).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CteDef {
    /// CTE alias name.
    pub alias: String,
    /// The subquery (cold query definition).
    pub subquery: Box<ColdQuery>,
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

/// Window function definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowFuncDef {
    /// Function: "ROW_NUMBER", "RANK", "DENSE_RANK", "SUM", "AVG", "COUNT",
    /// "LEAD", "LAG", "FIRST_VALUE", "LAST_VALUE".
    pub func: String,
    /// Column for aggregate window functions (None for ROW_NUMBER, RANK, etc.).
    pub column: Option<String>,
    /// PARTITION BY columns.
    #[serde(default)]
    pub partition_by: Vec<String>,
    /// ORDER BY within partition.
    #[serde(default)]
    pub order_by: Vec<ColdOrder>,
    /// Output alias; defaults to "{func}()" if None.
    pub alias: Option<String>,
}

/// JSON column extraction in projections.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonProjection {
    /// Source column containing JSON (e.g. "metadata").
    pub column: String,
    /// JSON path (e.g. "$.category" or "category").
    pub path: String,
    /// Optional output alias.
    pub alias: Option<String>,
}

/// JSON property path filter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonFilter {
    /// Source column containing JSON.
    pub column: String,
    /// JSON path (e.g. "$.domain").
    pub path: String,
    /// Operator: "Eq", "Ne", "Gt", "Lt", "Gte", "Lte", "In", "Contains".
    pub op: String,
    /// Comparison value.
    pub value: Value,
}

/// Vector similarity threshold filter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorFilter {
    /// Column containing the vector (FLOAT[] type).
    pub column: String,
    /// Distance metric: "cosine" or "euclidean".
    pub metric: String,
    /// Query vector.
    pub vector: Vec<f64>,
    /// Comparison operator: "Gte" for >=, "Lte" for <=.
    /// Cosine similarity uses Gte (closer to 1 = more similar).
    /// Euclidean distance uses Lte (smaller = more similar).
    pub op: String,
    /// Threshold value.
    pub threshold: f64,
}

/// Vector similarity score projection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorScore {
    /// Column containing the vector.
    pub column: String,
    /// Distance metric: "cosine" or "euclidean".
    pub metric: String,
    /// Query vector.
    pub vector: Vec<f64>,
    /// Output alias for the score column (defaults to "score").
    pub alias: Option<String>,
    /// If true, sorts by score descending (cosine) or ascending (euclidean).
    pub sort_by_score: bool,
}
