// DuckDB Cypher->SQL translator.
//
// Translates a simple intermediate query description (ColdQuery) into a DuckDB
// SQL SELECT statement against parquet-backed FIH views (facts_view,
// intents_view, hints_view).
//
// ColdQuery is a JSON-serializable struct that the graph layer's Cypher executor
// produces when it determines a query cannot benefit from graph traversal and
// should be routed to cold storage.
//
// Supported features:
//   - Tabular scans (Fact, Intent, Hint) with column projections
//   - Property filters (Eq, Ne, Gt, Lt, Gte, Lte, In, Contains, FtsMatch)
//   - JSON property filters (JsonEq, JsonNe, JsonGt, etc.)
//   - Vector similarity filters (array_cosine_similarity, array_distance)
//   - Vector score projection (with ORDER BY score DESC)
//   - JSON column extraction in projections
//   - GROUP BY with aggregates (SUM, AVG, MIN, MAX, COUNT, COUNT_DISTINCT)
//   - Window functions (ROW_NUMBER, RANK, DENSE_RANK, SUM, AVG, COUNT)
//   - CTE / WITH clause support
//   - ORDER BY, LIMIT, OFFSET, DISTINCT

use crate::duckdb_ext::{DuckDbQueryExt, JsonFilter, VectorFilter, WindowFuncDef};
use interface_query::{ColdFilter, ColdQuery};
use serde_json::Value;

// ── SQL translation ────────────────────────────────────────────────────────

/// Translate a ColdQuery into a DuckDB SQL SELECT string.
pub fn translate(query: &ColdQuery, ext: Option<&DuckDbQueryExt>) -> Result<String, String> {
    let default_ext;
    let ext = match ext {
        Some(e) => e,
        None => {
            default_ext = DuckDbQueryExt::default();
            &default_ext
        }
    };

    let table = resolve_table(&query.label)?;

    // 1. CTE prefix
    let cte_prefix = if ext.with_ctes.is_empty() {
        String::new()
    } else {
        let cte_strings: Vec<String> = ext
            .with_ctes
            .iter()
            .map(|cte| -> Result<String, String> {
                let sub_sql = translate(&cte.subquery, None)?;
                Ok(format!("{} AS ({})", quote_ident(&cte.alias), sub_sql))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let joined = cte_strings.join(", ");
        format!("WITH {} ", joined)
    };

    // 2. SELECT clause
    let select_clause = build_select_clause(query, ext)?;

    // 3. FROM clause
    let mut sql = format!("{cte_prefix}{select_clause} FROM {table}");

    // 4. WHERE clause (regular filters + json filters + vector filters)
    let where_parts = build_where_clause(query, table, ext)?;

    if !where_parts.is_empty() {
        sql = format!("{} WHERE {}", sql, where_parts.join(" AND "));
    }

    // 5. GROUP BY
    if !query.group_by.is_empty() {
        let cols: Vec<String> = query.group_by.iter().map(|c| quote_ident(c)).collect();
        sql = format!("{} GROUP BY {}", sql, cols.join(", "));
    }

    // 6. ORDER BY
    let order_clause = build_order_clause(query, ext)?;
    if let Some(oc) = order_clause {
        sql = format!("{} ORDER BY {}", sql, oc);
    }

    // 7. LIMIT / OFFSET
    sql = apply_limit_offset(query, sql);

    Ok(sql)
}

// ── Query building helpers ─────────────────────────────────────────────────

/// Resolve the label string to a DuckDB view name.
fn resolve_table(label: &str) -> Result<&'static str, String> {
    match label {
        "Fact" => Ok("facts_view"),
        "Intent" => Ok("intents_view"),
        "Hint" => Ok("hints_view"),
        other => Err(format!(
            "unknown label '{other}': expected Fact, Intent, or Hint"
        )),
    }
}

/// Build the SELECT clause string.
fn build_select_clause(query: &ColdQuery, ext: &DuckDbQueryExt) -> Result<String, String> {
    let mut select_parts: Vec<String> = Vec::new();

    // Aggregate count overrides everything
    if query.aggregate_count {
        return Ok("SELECT COUNT(*) as count".to_string());
    }

    // Regular column projections
    for col in &query.projections {
        select_parts.push(quote_ident(col));
    }

    // JSON extractions
    for jp in &ext.json_projections {
        let alias = jp
            .alias
            .as_deref()
            .unwrap_or(jp.path.trim_start_matches("$."));
        select_parts.push(format!(
            "json_extract_string({}, '{}') AS {}",
            quote_ident(&jp.column),
            jp.path,
            quote_ident(alias)
        ));
    }

    // Aggregate projections
    for agg in &query.aggregates {
        let alias = agg
            .alias
            .clone()
            .unwrap_or_else(|| format!("{}({})", agg.func.to_lowercase(), agg.column));
        let col_ref = if agg.column == "*" {
            "*".to_string()
        } else {
            quote_ident(&agg.column)
        };
        let sql_func = match agg.func.to_uppercase().as_str() {
            "COUNT_DISTINCT" => format!("COUNT(DISTINCT {})", col_ref),
            other => format!("{}({})", other, col_ref),
        };
        select_parts.push(format!("{} AS {}", sql_func, quote_ident(&alias)));
    }

    // Window function projections
    for wf in &ext.window_funcs {
        let alias = wf
            .alias
            .clone()
            .unwrap_or_else(|| format!("{}_window", wf.func.to_lowercase()));
        let over_clause = build_window_over_clause(wf);
        let col_expr = wf.column.as_deref().map(quote_ident).unwrap_or_default();
        let sql_func = match wf.func.to_uppercase().as_str() {
            "ROW_NUMBER" => "ROW_NUMBER()".to_string(),
            "RANK" => "RANK()".to_string(),
            "DENSE_RANK" => "DENSE_RANK()".to_string(),
            "LEAD" => format!("LEAD({})", col_expr),
            "LAG" => format!("LAG({})", col_expr),
            "FIRST_VALUE" => format!("FIRST_VALUE({})", col_expr),
            "LAST_VALUE" => format!("LAST_VALUE({})", col_expr),
            other => format!("{}({})", other, col_expr),
        };
        select_parts.push(format!(
            "{} OVER ({}) AS {}",
            sql_func,
            over_clause,
            quote_ident(&alias)
        ));
    }

    // Vector score projection
    if let Some(vs) = &ext.vector_score {
        // When projections is empty, we still want all columns plus the score column.
        let alias = vs.alias.as_deref().unwrap_or("score");
        let func_name = match vs.metric.to_lowercase().as_str() {
            "cosine" => "array_cosine_similarity",
            "euclidean" => "array_distance",
            _ => {
                return Err(format!(
                    "unknown vector metric '{}': expected cosine or euclidean",
                    vs.metric
                ));
            }
        };
        let vec_literal = format_vector_literal(&vs.vector);
        let score_col = format!(
            "{}({}, {}) AS {}",
            func_name,
            quote_ident(&vs.column),
            vec_literal,
            quote_ident(alias)
        );
        // If no regular projections, prepend * so we get all columns plus score.
        if select_parts.is_empty() {
            select_parts.push("*".to_string());
        }
        select_parts.push(score_col);
    }

    // If nothing selected, default to all columns
    if select_parts.is_empty() {
        if query.distinct {
            return Ok("SELECT DISTINCT *".to_string());
        }
        return Ok("SELECT *".to_string());
    }

    let cols = select_parts.join(", ");
    if query.distinct {
        Ok(format!("SELECT DISTINCT {cols}"))
    } else {
        Ok(format!("SELECT {cols}"))
    }
}

/// Build the OVER clause for a window function.
fn build_window_over_clause(wf: &WindowFuncDef) -> String {
    let mut parts: Vec<String> = Vec::new();

    if !wf.partition_by.is_empty() {
        let pcols: Vec<String> = wf.partition_by.iter().map(|c| quote_ident(c)).collect();
        parts.push(format!("PARTITION BY {}", pcols.join(", ")));
    }

    if !wf.order_by.is_empty() {
        let ocols: Vec<String> = wf
            .order_by
            .iter()
            .map(|o| {
                let dir = if o.desc { "DESC" } else { "ASC" };
                format!("{} {}", quote_ident(&o.field), dir)
            })
            .collect();
        parts.push(format!("ORDER BY {}", ocols.join(", ")));
    }

    parts.join(" ")
}

/// Build the ORDER BY clause.
fn build_order_clause(query: &ColdQuery, ext: &DuckDbQueryExt) -> Result<Option<String>, String> {
    let mut order_parts: Vec<String> = Vec::new();

    for o in &query.order_by {
        let dir = if o.desc { "DESC" } else { "ASC" };
        order_parts.push(format!("{} {}", quote_ident(&o.field), dir));
    }

    // Vector score ordering
    if let Some(vs) = &ext.vector_score
        && vs.sort_by_score
    {
        let alias = vs.alias.as_deref().unwrap_or("score");
        let dir = match vs.metric.to_lowercase().as_str() {
            "cosine" => "DESC",   // higher cosine = more similar
            "euclidean" => "ASC", // smaller distance = more similar
            _ => "DESC",
        };
        order_parts.push(format!("{} {}", quote_ident(alias), dir));
    }

    if order_parts.is_empty() {
        Ok(None)
    } else {
        Ok(Some(order_parts.join(", ")))
    }
}

/// Build WHERE clause parts: regular filters + json filters + vector filters.
fn build_where_clause(
    query: &ColdQuery,
    table: &str,
    ext: &DuckDbQueryExt,
) -> Result<Vec<String>, String> {
    let mut parts: Vec<String> = Vec::new();

    // Regular column filters
    for f in &query.filters {
        parts.push(translate_filter(f, table)?);
    }

    // JSON path filters
    for jf in &ext.json_filters {
        parts.push(translate_json_filter(jf)?);
    }

    // Vector similarity filters
    for vf in &ext.vector_filters {
        parts.push(translate_vector_filter(vf)?);
    }

    Ok(parts)
}

/// Translate a single filter condition to a SQL WHERE fragment.
fn translate_filter(f: &ColdFilter, table: &str) -> Result<String, String> {
    let col = &f.field;
    let val_str = value_to_sql(&f.value);

    match f.op.as_str() {
        "Eq" => Ok(format!("{col} = {val_str}")),
        "Ne" => Ok(format!("{col} != {val_str}")),
        "Gt" => Ok(format!("{col} > {val_str}")),
        "Lt" => Ok(format!("{col} < {val_str}")),
        "Gte" => Ok(format!("{col} >= {val_str}")),
        "Lte" => Ok(format!("{col} <= {val_str}")),
        "In" => {
            let arr = f
                .value
                .as_array()
                .ok_or_else(|| "In filter requires an array value".to_string())?;
            let items: Vec<String> = arr.iter().map(value_to_sql).collect();
            Ok(format!("{col} IN ({})", items.join(", ")))
        }
        "Contains" => Ok(format!("CONTAINS({col}, {val_str})")),
        "FtsMatch" => {
            // Full-text search via DuckDB FTS extension.
            // Assumes an FTS index exists as fts_main_{table}.
            let fts_table = fts_index_name(table);
            Ok(format!(
                "{col} IN (SELECT doc_id FROM {fts_table} WHERE {fts_table}.match({val_str}))"
            ))
        }
        "FtsMatchAnd" => {
            // Multi-term AND match: every term must appear somewhere.
            // Splits terms and uses CONTAINS with AND (no FTS index required).
            let terms = extract_terms(&val_str);
            if terms.len() <= 1 {
                return Ok(format!("CONTAINS({col}, {val_str})"));
            }
            let and_clauses: Vec<String> = terms
                .iter()
                .map(|t| {
                    format!(
                        "CONTAINS({col}, {})",
                        value_to_sql(&Value::String(t.to_string()))
                    )
                })
                .collect();
            Ok(format!("({})", and_clauses.join(" AND ")))
        }
        "FtsMatchOr" => {
            // Multi-term OR match: any term suffices.
            // Convert to stem-based CONTAINS with OR.
            let terms = extract_terms(&val_str);
            if terms.len() <= 1 {
                return Ok(format!("CONTAINS({col}, {val_str})"));
            }
            let or_clauses: Vec<String> = terms
                .iter()
                .map(|t| {
                    format!(
                        "CONTAINS({col}, {})",
                        value_to_sql(&Value::String(t.to_string()))
                    )
                })
                .collect();
            Ok(format!("({})", or_clauses.join(" OR ")))
        }
        o => Err(format!("unknown filter operator '{o}'")),
    }
}

/// Translate a JSON filter to SQL WHERE fragment.
fn translate_json_filter(jf: &JsonFilter) -> Result<String, String> {
    let extract = format!(
        "json_extract_string({}, '{}')",
        quote_ident(&jf.column),
        jf.path
    );
    let val_str = value_to_sql(&jf.value);

    match jf.op.as_str() {
        "Eq" => Ok(format!("{extract} = {val_str}")),
        "Ne" => Ok(format!("{extract} != {val_str}")),
        "Gt" => Ok(format!("{extract} > {val_str}")),
        "Lt" => Ok(format!("{extract} < {val_str}")),
        "Gte" => Ok(format!("{extract} >= {val_str}")),
        "Lte" => Ok(format!("{extract} <= {val_str}")),
        "In" => {
            let arr = jf
                .value
                .as_array()
                .ok_or_else(|| "In filter requires an array value".to_string())?;
            let items: Vec<String> = arr.iter().map(value_to_sql).collect();
            Ok(format!("{extract} IN ({})", items.join(", ")))
        }
        "Contains" => Ok(format!("CONTAINS({extract}, {val_str})")),
        o => Err(format!("unknown json filter operator '{o}'")),
    }
}

/// Translate a vector similarity filter to SQL WHERE fragment.
fn translate_vector_filter(vf: &VectorFilter) -> Result<String, String> {
    let vec_literal = format_vector_literal(&vf.vector);
    let func_name = match vf.metric.to_lowercase().as_str() {
        "cosine" => "array_cosine_similarity",
        "euclidean" => "array_distance",
        m => return Err(format!("unknown vector metric '{m}'")),
    };
    let expr = format!(
        "{}({}, {})",
        func_name,
        quote_ident(&vf.column),
        vec_literal
    );

    match vf.op.as_str() {
        "Gte" => Ok(format!("{expr} >= {}", vf.threshold)),
        "Lte" => Ok(format!("{expr} <= {}", vf.threshold)),
        o => Err(format!(
            "unknown vector filter operator '{o}': expected Gte or Lte"
        )),
    }
}

/// Apply LIMIT / OFFSET to a SQL string.
fn apply_limit_offset(query: &ColdQuery, sql: String) -> String {
    match (query.limit, query.offset) {
        (Some(limit), Some(offset)) => format!("{sql} LIMIT {limit} OFFSET {offset}"),
        (Some(limit), None) => format!("{sql} LIMIT {limit}"),
        (None, Some(offset)) => format!("{sql} LIMIT 1000000 OFFSET {offset}"),
        (None, None) => sql,
    }
}

// ── Helper utilities ───────────────────────────────────────────────────────

/// Quote a SQL identifier (column or alias) to handle reserved words.
///
/// Qualified names containing dots (e.g. `table.column`) are split and each
/// segment is quoted separately, producing `"table"."column"`.
pub fn quote_ident(name: &str) -> String {
    // If already fully quoted, return as-is.
    if name.starts_with('"') && name.ends_with('"') {
        return name.to_string();
    }
    // Qualified identifier: split on dot, quote each segment.
    if name.contains('.') {
        let parts: Vec<String> = name
            .split('.')
            .map(|seg| {
                let seg = seg.trim();
                quote_ident(seg)
            })
            .collect();
        return parts.join(".");
    }
    // If it contains special characters, quote it.
    if name.chars().any(|c| !c.is_ascii_alphanumeric() && c != '_') {
        format!("\"{}\"", name.replace('"', "\"\""))
    } else {
        name.to_string()
    }
}

/// Derive the FTS index table name from a view name.
pub fn fts_index_name(view_name: &str) -> String {
    // DuckDB FTS extension names the index table fts_main_{view_name}.
    // Strip trailing "_view" suffix if present for brevity.
    let base = view_name.strip_suffix("_view").unwrap_or(view_name);
    format!("fts_main_{base}")
}

/// Format a Rust float vector as a DuckDB array literal.
fn format_vector_literal(vec: &[f64]) -> String {
    let items: Vec<String> = vec
        .iter()
        .map(|v| {
            if v.fract() == 0.0 && v.is_finite() {
                format!("{:.1}", v)
            } else {
                v.to_string()
            }
        })
        .collect();
    format!("[{}]", items.join(", "))
}

/// Extract individual terms (words) from a SQL string literal value.
/// Strips surrounding single quotes.
pub fn extract_terms(val_str: &str) -> Vec<String> {
    let stripped = val_str.trim_matches('\'');
    stripped
        .split_whitespace()
        .map(|s| s.trim_matches(&['\'', '"', ',', '.', ';', ':', '!', '?'][..]))
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// Format a serde_json::Value as a DuckDB SQL literal.
pub fn value_to_sql(v: &Value) -> String {
    match v {
        Value::String(s) => {
            let escaped = s.replace('\'', "''");
            format!("'{}'", escaped)
        }
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => {
            if *b {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        }
        Value::Null => "NULL".to_string(),
        Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(value_to_sql).collect();
            format!("({})", items.join(", "))
        }
        Value::Object(_) => {
            let s = serde_json::to_string(v).unwrap_or_default();
            let escaped = s.replace('\'', "''");
            format!("'{}'", escaped)
        }
    }
}
