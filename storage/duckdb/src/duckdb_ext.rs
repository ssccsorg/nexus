// DuckDB-specific query extension types.
//
// These types represent DuckDB-specific SQL features that are not part of the
// generic tabular query model. They are bundled in `DuckDbQueryExt` and passed
// to `cypher_sql::translate` alongside the generic `ColdQuery`.

use interface_query::{ColdOrder, ColdQuery};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// DuckDB-specific extensions to a generic tabular query.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DuckDbQueryExt {
    /// CTE (WITH clause) definitions.
    #[serde(default)]
    pub with_ctes: Vec<CteDef>,
    /// Window function projections.
    #[serde(default)]
    pub window_funcs: Vec<WindowFuncDef>,
    /// JSON column extractions in the SELECT clause.
    #[serde(default)]
    pub json_projections: Vec<JsonProjection>,
    /// JSON path filters in the WHERE clause.
    #[serde(default)]
    pub json_filters: Vec<JsonFilter>,
    /// Vector similarity filters in the WHERE clause.
    #[serde(default)]
    pub vector_filters: Vec<VectorFilter>,
    /// Vector score projection plus optional ORDER BY.
    pub vector_score: Option<VectorScore>,
}

/// A CTE (Common Table Expression) definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CteDef {
    /// Alias name for the CTE.
    pub alias: String,
    /// The subquery.
    pub subquery: Box<ColdQuery>,
}

/// A window function projection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowFuncDef {
    /// Window function name: "ROW_NUMBER", "RANK", "DENSE_RANK", "SUM", "AVG", "COUNT",
    /// "LEAD", "LAG", "FIRST_VALUE", "LAST_VALUE".
    pub func: String,
    /// Target column expression (None for parameterless functions like ROW_NUMBER).
    pub column: Option<String>,
    /// PARTITION BY columns.
    #[serde(default)]
    pub partition_by: Vec<String>,
    /// ORDER BY within the window.
    #[serde(default)]
    pub order_by: Vec<ColdOrder>,
    /// Output alias; defaults to "{func}_window" if None.
    pub alias: Option<String>,
}

/// A JSON column extraction in the SELECT clause.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonProjection {
    /// Source column containing JSON text.
    pub column: String,
    /// JSON path to extract (e.g. "$.category").
    pub path: String,
    /// Output alias; defaults to path sans "$." if None.
    pub alias: Option<String>,
}

/// A JSON path filter in the WHERE clause.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonFilter {
    /// Source column containing JSON text.
    pub column: String,
    /// JSON path to filter on.
    pub path: String,
    /// Operator: "Eq", "Ne", "Gt", "Lt", "Gte", "Lte", "In", "Contains".
    pub op: String,
    /// Operand value.
    pub value: Value,
}

/// A vector similarity filter in the WHERE clause.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorFilter {
    /// Column containing the embedding vector.
    pub column: String,
    /// Similarity metric: "cosine" or "euclidean".
    pub metric: String,
    /// Query vector.
    pub vector: Vec<f64>,
    /// Operator: "Gte" or "Lte".
    pub op: String,
    /// Similarity threshold.
    pub threshold: f64,
}

/// Vector score projection (adds a similarity/distance column to SELECT).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorScore {
    /// Column containing the embedding vector.
    pub column: String,
    /// Similarity metric: "cosine" or "euclidean".
    pub metric: String,
    /// Query vector.
    pub vector: Vec<f64>,
    /// Output alias; defaults to "score" if None.
    pub alias: Option<String>,
    /// When true, append ORDER BY score DESC (cosine) or ASC (euclidean).
    pub sort_by_score: bool,
}
