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

use nexus_graph::query::cypher::cold_query::{ColdFilter, ColdQuery, JsonFilter, VectorFilter, WindowFuncDef};
use serde_json::Value;

// ── SQL translation ────────────────────────────────────────────────────────

/// Translate a ColdQuery into a DuckDB SQL SELECT string.
pub fn translate(query: &ColdQuery) -> Result<String, String> {
    let table = resolve_table(&query.label)?;

    // 1. CTE prefix
    let cte_prefix = if query.with_ctes.is_empty() {
        String::new()
    } else {
        let cte_strings: Vec<String> = query
            .with_ctes
            .iter()
            .map(|cte| -> Result<String, String> {
                let sub_sql = translate(&cte.subquery)?;
                Ok(format!("{} AS ({})", quote_ident(&cte.alias), sub_sql))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let joined = cte_strings.join(", ");
        format!("WITH {} ", joined)
    };

    // 2. SELECT clause
    let select_clause = build_select_clause(query)?;

    // 3. FROM clause
    let mut sql = format!("{cte_prefix}{select_clause} FROM {table}");

    // 4. WHERE clause (regular filters + json filters + vector filters)
    let where_parts = build_where_clause(query, table)?;

    if !where_parts.is_empty() {
        sql = format!("{} WHERE {}", sql, where_parts.join(" AND "));
    }

    // 5. GROUP BY
    if !query.group_by.is_empty() {
        let cols: Vec<String> = query.group_by.iter().map(|c| quote_ident(c)).collect();
        sql = format!("{} GROUP BY {}", sql, cols.join(", "));
    }

    // 6. ORDER BY
    let order_clause = build_order_clause(query)?;
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
fn build_select_clause(query: &ColdQuery) -> Result<String, String> {
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
    for jp in &query.json_projections {
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
    for wf in &query.window_funcs {
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
    if let Some(vs) = &query.vector_score {
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
fn build_order_clause(query: &ColdQuery) -> Result<Option<String>, String> {
    let mut order_parts: Vec<String> = Vec::new();

    for o in &query.order_by {
        let dir = if o.desc { "DESC" } else { "ASC" };
        order_parts.push(format!("{} {}", quote_ident(&o.field), dir));
    }

    // Vector score ordering
    if let Some(vs) = &query.vector_score
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
fn build_where_clause(query: &ColdQuery, table: &str) -> Result<Vec<String>, String> {
    let mut parts: Vec<String> = Vec::new();

    // Regular column filters
    for f in &query.filters {
        parts.push(translate_filter(f, table)?);
    }

    // JSON path filters
    for jf in &query.json_filters {
        parts.push(translate_json_filter(jf)?);
    }

    // Vector similarity filters
    for vf in &query.vector_filters {
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
fn quote_ident(name: &str) -> String {
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
fn fts_index_name(view_name: &str) -> String {
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
fn extract_terms(val_str: &str) -> Vec<String> {
    let stripped = val_str.trim_matches('\'');
    stripped
        .split_whitespace()
        .map(|s| s.trim_matches(&['\'', '"', ',', '.', ';', ':', '!', '?'][..]))
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

/// Format a serde_json::Value as a DuckDB SQL literal.
fn value_to_sql(v: &Value) -> String {
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

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_graph::query::cypher::cold_query::{AggregateDef, ColdOrder, CteDef, JsonProjection, VectorScore};

    // ── Existing test helpers ──────────────────────────────────────────────

    fn base_query(label: &str) -> ColdQuery {
        ColdQuery {
            label: label.to_string(),
            filters: vec![],
            projections: vec![],
            order_by: vec![],
            limit: None,
            offset: None,
            distinct: false,
            aggregate_count: false,
            with_ctes: vec![],
            group_by: vec![],
            aggregates: vec![],
            window_funcs: vec![],
            json_projections: vec![],
            json_filters: vec![],
            vector_filters: vec![],
            vector_score: None,
        }
    }

    // ── Existing tests (unchanged semantics) ───────────────────────────────

    #[test]
    fn test_translate_fact_scan() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into(), "origin".into()];
        let sql = translate(&q).unwrap();
        assert_eq!(sql, "SELECT fact_id, origin FROM facts_view");
    }

    #[test]
    fn test_translate_intent_scan() {
        let mut q = base_query("Intent");
        q.projections = vec!["intent_id".into(), "description".into()];
        let sql = translate(&q).unwrap();
        assert_eq!(sql, "SELECT intent_id, description FROM intents_view");
    }

    #[test]
    fn test_translate_hint_scan() {
        let mut q = base_query("Hint");
        q.projections = vec!["hint_id".into(), "content".into()];
        let sql = translate(&q).unwrap();
        assert_eq!(sql, "SELECT hint_id, content FROM hints_view");
    }

    #[test]
    fn test_translate_all_columns() {
        let q = base_query("Fact");
        let sql = translate(&q).unwrap();
        assert_eq!(sql, "SELECT * FROM facts_view");
    }

    #[test]
    fn test_translate_filter_eq() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into()];
        q.filters = vec![ColdFilter {
            field: "origin".into(),
            op: "Eq".into(),
            value: Value::String("arxiv_2401".into()),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT fact_id FROM facts_view WHERE origin = 'arxiv_2401'"
        );
    }

    #[test]
    fn test_translate_filter_multiple() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into()];
        q.filters = vec![
            ColdFilter {
                field: "origin".into(),
                op: "Eq".into(),
                value: Value::String("test".into()),
            },
            ColdFilter {
                field: "creator".into(),
                op: "Eq".into(),
                value: Value::String("agent-a".into()),
            },
        ];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT fact_id FROM facts_view WHERE origin = 'test' AND creator = 'agent-a'"
        );
    }

    #[test]
    fn test_translate_order_limit_offset() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into()];
        q.order_by = vec![ColdOrder {
            field: "created_at".into(),
            desc: true,
        }];
        q.limit = Some(10);
        q.offset = Some(5);
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT fact_id FROM facts_view ORDER BY created_at DESC LIMIT 10 OFFSET 5"
        );
    }

    #[test]
    fn test_translate_distinct() {
        let mut q = base_query("Fact");
        q.projections = vec!["origin".into()];
        q.distinct = true;
        let sql = translate(&q).unwrap();
        assert_eq!(sql, "SELECT DISTINCT origin FROM facts_view");
    }

    #[test]
    fn test_translate_count() {
        let mut q = base_query("Fact");
        q.aggregate_count = true;
        let sql = translate(&q).unwrap();
        assert_eq!(sql, "SELECT COUNT(*) as count FROM facts_view");
    }

    #[test]
    fn test_translate_in_filter() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into()];
        q.filters = vec![ColdFilter {
            field: "fact_id".into(),
            op: "In".into(),
            value: Value::Array(vec![
                Value::String("f001".into()),
                Value::String("f002".into()),
            ]),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT fact_id FROM facts_view WHERE fact_id IN ('f001', 'f002')"
        );
    }

    #[test]
    fn test_translate_unknown_label() {
        let q = ColdQuery {
            label: "Unknown".into(),
            filters: vec![],
            projections: vec![],
            order_by: vec![],
            limit: None,
            offset: None,
            distinct: false,
            aggregate_count: false,
            with_ctes: vec![],
            group_by: vec![],
            aggregates: vec![],
            window_funcs: vec![],
            json_projections: vec![],
            json_filters: vec![],
            vector_filters: vec![],
            vector_score: None,
        };
        let result = translate(&q);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown label"));
    }

    #[test]
    fn test_translate_contains_filter() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into()];
        q.filters = vec![ColdFilter {
            field: "content".into(),
            op: "Contains".into(),
            value: Value::String("neural".into()),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT fact_id FROM facts_view WHERE CONTAINS(content, 'neural')"
        );
    }

    #[test]
    fn test_translate_unknown_operator() {
        let mut q = base_query("Fact");
        q.filters = vec![ColdFilter {
            field: "origin".into(),
            op: "Regex".into(),
            value: Value::String(".*".into()),
        }];
        let result = translate(&q);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown filter operator"));
    }

    // ── FTS (Full Text Search) ────────────────────────────────────────────

    #[test]
    fn test_fts_match_filter() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into()];
        q.filters = vec![ColdFilter {
            field: "content".into(),
            op: "FtsMatch".into(),
            value: Value::String("neural network".into()),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT fact_id FROM facts_view WHERE content IN (SELECT doc_id FROM fts_main_facts WHERE fts_main_facts.match('neural network'))"
        );
    }

    #[test]
    fn test_fts_match_intent() {
        let mut q = base_query("Intent");
        q.projections = vec!["intent_id".into()];
        q.filters = vec![ColdFilter {
            field: "description".into(),
            op: "FtsMatch".into(),
            value: Value::String("concept drift".into()),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT intent_id FROM intents_view WHERE description IN (SELECT doc_id FROM fts_main_intents WHERE fts_main_intents.match('concept drift'))"
        );
    }

    #[test]
    fn test_fts_match_or_filter() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into()];
        q.filters = vec![ColdFilter {
            field: "content".into(),
            op: "FtsMatchOr".into(),
            value: Value::String("neural transformer".into()),
        }];
        let sql = translate(&q).unwrap();
        // Two terms: CONTAINS(col, 'neural') OR CONTAINS(col, 'transformer')
        assert!(sql.contains("CONTAINS(content, 'neural') OR CONTAINS(content, 'transformer')"));
        assert!(sql.starts_with("SELECT fact_id FROM facts_view WHERE"));
    }

    #[test]
    fn test_fts_match_or_single_term_falls_back_to_contains() {
        let mut q = base_query("Fact");
        q.filters = vec![ColdFilter {
            field: "content".into(),
            op: "FtsMatchOr".into(),
            value: Value::String("neural".into()),
        }];
        let sql = translate(&q).unwrap();
        // Single term: just CONTAINS, no OR
        assert_eq!(
            sql,
            "SELECT * FROM facts_view WHERE CONTAINS(content, 'neural')"
        );
    }

    #[test]
    fn test_fts_filter_combined_with_eq() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into()];
        q.filters = vec![
            ColdFilter {
                field: "origin".into(),
                op: "Eq".into(),
                value: Value::String("arxiv".into()),
            },
            ColdFilter {
                field: "content".into(),
                op: "FtsMatch".into(),
                value: Value::String("deep learning".into()),
            },
        ];
        let sql = translate(&q).unwrap();
        assert!(sql.contains("origin = 'arxiv'"));
        assert!(sql.contains("content IN (SELECT doc_id FROM fts_main_facts WHERE fts_main_facts.match('deep learning'))"));
        assert!(sql.contains("AND"));
    }

    // ── Vector similarity filters ─────────────────────────────────────────

    #[test]
    fn test_vector_cosine_similarity_filter() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into()];
        q.vector_filters = vec![VectorFilter {
            column: "embedding".into(),
            metric: "cosine".into(),
            vector: vec![0.1, 0.2, 0.3],
            op: "Gte".into(),
            threshold: 0.8,
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT fact_id FROM facts_view WHERE array_cosine_similarity(embedding, [0.1, 0.2, 0.3]) >= 0.8"
        );
    }

    #[test]
    fn test_vector_euclidean_distance_filter() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into()];
        q.vector_filters = vec![VectorFilter {
            column: "embedding".into(),
            metric: "euclidean".into(),
            vector: vec![1.0, 2.0, 3.0],
            op: "Lte".into(),
            threshold: 5.0,
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT fact_id FROM facts_view WHERE array_distance(embedding, [1.0, 2.0, 3.0]) <= 5"
        );
    }

    #[test]
    fn test_vector_filter_multiple_and_conditions() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into()];
        q.filters = vec![ColdFilter {
            field: "origin".into(),
            op: "Eq".into(),
            value: Value::String("arxiv".into()),
        }];
        q.vector_filters = vec![VectorFilter {
            column: "embedding".into(),
            metric: "cosine".into(),
            vector: vec![0.5, 0.5],
            op: "Gte".into(),
            threshold: 0.9,
        }];
        let sql = translate(&q).unwrap();
        assert!(sql.contains("origin = 'arxiv'"));
        assert!(sql.contains("array_cosine_similarity(embedding, [0.5, 0.5]) >= 0.9"));
        assert!(sql.contains("AND"));
    }

    #[test]
    fn test_vector_filter_unknown_metric() {
        let mut q = base_query("Fact");
        q.vector_filters = vec![VectorFilter {
            column: "v".into(),
            metric: "manhattan".into(),
            vector: vec![1.0],
            op: "Gte".into(),
            threshold: 0.5,
        }];
        let result = translate(&q);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown vector metric"));
    }

    #[test]
    fn test_vector_filter_unknown_operator() {
        let mut q = base_query("Fact");
        q.vector_filters = vec![VectorFilter {
            column: "v".into(),
            metric: "cosine".into(),
            vector: vec![1.0],
            op: "Eq".into(),
            threshold: 0.5,
        }];
        let result = translate(&q);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("unknown vector filter operator")
        );
    }

    // ── Vector score projection ───────────────────────────────────────────

    #[test]
    fn test_vector_score_cosine() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into()];
        q.vector_score = Some(VectorScore {
            column: "embedding".into(),
            metric: "cosine".into(),
            vector: vec![0.1, 0.2, 0.3],
            alias: Some("similarity".into()),
            sort_by_score: true,
        });
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT fact_id, array_cosine_similarity(embedding, [0.1, 0.2, 0.3]) AS similarity FROM facts_view ORDER BY similarity DESC"
        );
    }

    #[test]
    fn test_vector_score_euclidean() {
        let mut q = base_query("Fact");
        q.vector_score = Some(VectorScore {
            column: "embedding".into(),
            metric: "euclidean".into(),
            vector: vec![1.0, 2.0, 3.0],
            alias: None,
            sort_by_score: true,
        });
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT *, array_distance(embedding, [1.0, 2.0, 3.0]) AS score FROM facts_view ORDER BY score ASC"
        );
    }

    #[test]
    fn test_vector_score_without_sort() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into()];
        q.vector_score = Some(VectorScore {
            column: "embedding".into(),
            metric: "cosine".into(),
            vector: vec![0.1, 0.2, 0.3],
            alias: None,
            sort_by_score: false,
        });
        let sql = translate(&q).unwrap();
        // Score column projected but no ORDER BY appended
        assert_eq!(
            sql,
            "SELECT fact_id, array_cosine_similarity(embedding, [0.1, 0.2, 0.3]) AS score FROM facts_view"
        );
    }

    #[test]
    fn test_vector_score_with_custom_order_by() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into()];
        q.order_by = vec![ColdOrder {
            field: "created_at".into(),
            desc: true,
        }];
        q.vector_score = Some(VectorScore {
            column: "embedding".into(),
            metric: "cosine".into(),
            vector: vec![0.1, 0.2, 0.3],
            alias: None,
            sort_by_score: true,
        });
        let sql = translate(&q).unwrap();
        // Both ORDER BY clauses present
        assert!(sql.contains("ORDER BY created_at DESC, score DESC"));
        assert!(sql.contains("array_cosine_similarity(embedding, [0.1, 0.2, 0.3]) AS score"));
    }

    #[test]
    fn test_vector_score_unknown_metric() {
        let mut q = base_query("Fact");
        q.vector_score = Some(VectorScore {
            column: "embedding".into(),
            metric: "dot_product".into(),
            vector: vec![1.0],
            alias: None,
            sort_by_score: false,
        });
        let result = translate(&q);
        assert!(result.is_err());
    }

    // ── JSON projections ──────────────────────────────────────────────────

    #[test]
    fn test_json_extract_projection() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into()];
        q.json_projections = vec![JsonProjection {
            column: "metadata".into(),
            path: "$.category".into(),
            alias: Some("category".into()),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT fact_id, json_extract_string(metadata, '$.category') AS category FROM facts_view"
        );
    }

    #[test]
    fn test_json_extract_multiple() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into()];
        q.json_projections = vec![
            JsonProjection {
                column: "metadata".into(),
                path: "$.domain".into(),
                alias: Some("domain".into()),
            },
            JsonProjection {
                column: "metadata".into(),
                path: "$.version".into(),
                alias: Some("version".into()),
            },
        ];
        let sql = translate(&q).unwrap();
        assert!(sql.contains("json_extract_string(metadata, '$.domain') AS domain"));
        assert!(sql.contains("json_extract_string(metadata, '$.version') AS version"));
    }

    #[test]
    fn test_json_extract_only() {
        let mut q = base_query("Fact");
        q.json_projections = vec![JsonProjection {
            column: "metadata".into(),
            path: "category".into(),
            alias: None,
        }];
        let sql = translate(&q).unwrap();
        // alias defaults to path sans "$."
        assert_eq!(
            sql,
            "SELECT json_extract_string(metadata, 'category') AS category FROM facts_view"
        );
    }

    // ── JSON filters ──────────────────────────────────────────────────────

    #[test]
    fn test_json_filter_eq() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into()];
        q.json_filters = vec![JsonFilter {
            column: "metadata".into(),
            path: "$.domain".into(),
            op: "Eq".into(),
            value: Value::String("mathematics".into()),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT fact_id FROM facts_view WHERE json_extract_string(metadata, '$.domain') = 'mathematics'"
        );
    }

    #[test]
    fn test_json_filter_gt() {
        let mut q = base_query("Fact");
        q.json_filters = vec![JsonFilter {
            column: "metadata".into(),
            path: "$.score".into(),
            op: "Gt".into(),
            value: Value::Number(serde_json::Number::from(85)),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM facts_view WHERE json_extract_string(metadata, '$.score') > 85"
        );
    }

    #[test]
    fn test_json_filter_contains() {
        let mut q = base_query("Fact");
        q.json_filters = vec![JsonFilter {
            column: "metadata".into(),
            path: "$.tags".into(),
            op: "Contains".into(),
            value: Value::String("graph".into()),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM facts_view WHERE CONTAINS(json_extract_string(metadata, '$.tags'), 'graph')"
        );
    }

    #[test]
    fn test_json_filter_combined_with_column_filter() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into()];
        q.filters = vec![ColdFilter {
            field: "origin".into(),
            op: "Eq".into(),
            value: Value::String("arxiv".into()),
        }];
        q.json_filters = vec![JsonFilter {
            column: "metadata".into(),
            path: "$.domain".into(),
            op: "Eq".into(),
            value: Value::String("cs.AI".into()),
        }];
        let sql = translate(&q).unwrap();
        assert!(sql.contains("origin = 'arxiv'"));
        assert!(sql.contains("json_extract_string(metadata, '$.domain') = 'cs.AI'"));
        assert!(sql.contains("AND"));
    }

    #[test]
    fn test_json_filter_in() {
        let mut q = base_query("Fact");
        q.json_filters = vec![JsonFilter {
            column: "metadata".into(),
            path: "$.category".into(),
            op: "In".into(),
            value: Value::Array(vec![
                Value::String("math".into()),
                Value::String("physics".into()),
            ]),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM facts_view WHERE json_extract_string(metadata, '$.category') IN ('math', 'physics')"
        );
    }

    #[test]
    fn test_json_filter_unknown_operator() {
        let mut q = base_query("Fact");
        q.json_filters = vec![JsonFilter {
            column: "metadata".into(),
            path: "$.x".into(),
            op: "Regex".into(),
            value: Value::String(".*".into()),
        }];
        let result = translate(&q);
        assert!(result.is_err());
    }

    // ── GROUP BY + Aggregates ─────────────────────────────────────────────

    #[test]
    fn test_group_by_single_column() {
        let mut q = base_query("Fact");
        q.projections = vec!["origin".into()];
        q.group_by = vec!["origin".into()];
        q.aggregates = vec![AggregateDef {
            func: "COUNT".into(),
            column: "fact_id".into(),
            alias: Some("cnt".into()),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT origin, COUNT(fact_id) AS cnt FROM facts_view GROUP BY origin"
        );
    }

    #[test]
    fn test_group_by_multiple_columns() {
        let mut q = base_query("Fact");
        q.projections = vec!["origin".into(), "creator".into()];
        q.group_by = vec!["origin".into(), "creator".into()];
        q.aggregates = vec![AggregateDef {
            func: "SUM".into(),
            column: "score".into(),
            alias: Some("total_score".into()),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT origin, creator, SUM(score) AS total_score FROM facts_view GROUP BY origin, creator"
        );
    }

    #[test]
    fn test_aggregate_count_distinct() {
        let mut q = base_query("Fact");
        q.projections = vec!["origin".into()];
        q.group_by = vec!["origin".into()];
        q.aggregates = vec![AggregateDef {
            func: "COUNT_DISTINCT".into(),
            column: "creator".into(),
            alias: Some("unique_creators".into()),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT origin, COUNT(DISTINCT creator) AS unique_creators FROM facts_view GROUP BY origin"
        );
    }

    #[test]
    fn test_aggregate_avg_min_max() {
        let mut q = base_query("Fact");
        q.projections = vec!["origin".into()];
        q.group_by = vec!["origin".into()];
        q.aggregates = vec![
            AggregateDef {
                func: "AVG".into(),
                column: "score".into(),
                alias: Some("avg_score".into()),
            },
            AggregateDef {
                func: "MIN".into(),
                column: "score".into(),
                alias: Some("min_score".into()),
            },
            AggregateDef {
                func: "MAX".into(),
                column: "score".into(),
                alias: Some("max_score".into()),
            },
        ];
        let sql = translate(&q).unwrap();
        assert!(sql.contains("AVG(score) AS avg_score"));
        assert!(sql.contains("MIN(score) AS min_score"));
        assert!(sql.contains("MAX(score) AS max_score"));
        assert!(sql.contains("GROUP BY origin"));
    }

    #[test]
    fn test_group_by_with_filter() {
        let mut q = base_query("Fact");
        q.projections = vec!["origin".into()];
        q.group_by = vec!["origin".into()];
        q.aggregates = vec![AggregateDef {
            func: "COUNT".into(),
            column: "fact_id".into(),
            alias: Some("cnt".into()),
        }];
        q.filters = vec![ColdFilter {
            field: "origin".into(),
            op: "Ne".into(),
            value: Value::String("test".into()),
        }];
        let sql = translate(&q).unwrap();
        assert!(sql.contains("WHERE origin != 'test'"));
        assert!(sql.contains("GROUP BY origin"));
    }

    #[test]
    fn test_aggregate_with_default_alias() {
        let mut q = base_query("Fact");
        q.projections = vec!["origin".into()];
        q.group_by = vec!["origin".into()];
        q.aggregates = vec![AggregateDef {
            func: "COUNT".into(),
            column: "fact_id".into(),
            alias: None,
        }];
        let sql = translate(&q).unwrap();
        // Default alias: "count(fact_id)"
        assert!(sql.contains("COUNT(fact_id) AS \"count(fact_id)\""));
    }

    // ── Window functions ──────────────────────────────────────────────────

    #[test]
    fn test_window_row_number() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into(), "origin".into()];
        q.window_funcs = vec![WindowFuncDef {
            func: "ROW_NUMBER".into(),
            column: None,
            partition_by: vec!["origin".into()],
            order_by: vec![ColdOrder {
                field: "created_at".into(),
                desc: true,
            }],
            alias: Some("rn".into()),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT fact_id, origin, ROW_NUMBER() OVER (PARTITION BY origin ORDER BY created_at DESC) AS rn FROM facts_view"
        );
    }

    #[test]
    fn test_window_rank() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into()];
        q.window_funcs = vec![WindowFuncDef {
            func: "RANK".into(),
            column: None,
            partition_by: vec![],
            order_by: vec![ColdOrder {
                field: "score".into(),
                desc: true,
            }],
            alias: Some("rank".into()),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT fact_id, RANK() OVER (ORDER BY score DESC) AS rank FROM facts_view"
        );
    }

    #[test]
    fn test_window_dense_rank() {
        let mut q = base_query("Fact");
        q.window_funcs = vec![WindowFuncDef {
            func: "DENSE_RANK".into(),
            column: None,
            partition_by: vec![],
            order_by: vec![],
            alias: None,
        }];
        let sql = translate(&q).unwrap();
        assert!(sql.contains("DENSE_RANK() OVER () AS dense_rank_window"));
    }

    #[test]
    fn test_window_sum_partition() {
        let mut q = base_query("Fact");
        q.projections = vec!["origin".into(), "score".into()];
        q.window_funcs = vec![WindowFuncDef {
            func: "SUM".into(),
            column: Some("score".into()),
            partition_by: vec!["origin".into()],
            order_by: vec![],
            alias: Some("origin_total".into()),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT origin, score, SUM(score) OVER (PARTITION BY origin) AS origin_total FROM facts_view"
        );
    }

    #[test]
    fn test_window_lead_lag() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into(), "created_at".into()];
        q.window_funcs = vec![
            WindowFuncDef {
                func: "LAG".into(),
                column: Some("created_at".into()),
                partition_by: vec!["origin".into()],
                order_by: vec![ColdOrder {
                    field: "created_at".into(),
                    desc: false,
                }],
                alias: Some("prev_created".into()),
            },
            WindowFuncDef {
                func: "LEAD".into(),
                column: Some("created_at".into()),
                partition_by: vec!["origin".into()],
                order_by: vec![ColdOrder {
                    field: "created_at".into(),
                    desc: false,
                }],
                alias: Some("next_created".into()),
            },
        ];
        let sql = translate(&q).unwrap();
        assert!(sql.contains(
            "LAG(created_at) OVER (PARTITION BY origin ORDER BY created_at ASC) AS prev_created"
        ));
        assert!(sql.contains(
            "LEAD(created_at) OVER (PARTITION BY origin ORDER BY created_at ASC) AS next_created"
        ));
    }

    #[test]
    fn test_window_first_last_value() {
        let mut q = base_query("Fact");
        q.projections = vec!["origin".into()];
        q.window_funcs = vec![WindowFuncDef {
            func: "FIRST_VALUE".into(),
            column: Some("fact_id".into()),
            partition_by: vec!["origin".into()],
            order_by: vec![ColdOrder {
                field: "created_at".into(),
                desc: false,
            }],
            alias: Some("first_fact".into()),
        }];
        let sql = translate(&q).unwrap();
        assert!(sql.contains(
            "FIRST_VALUE(fact_id) OVER (PARTITION BY origin ORDER BY created_at ASC) AS first_fact"
        ));
    }

    // ── CTE (WITH clause) ─────────────────────────────────────────────────

    #[test]
    fn test_cte_simple() {
        let sub = Box::new(ColdQuery {
            label: "Fact".into(),
            filters: vec![ColdFilter {
                field: "origin".into(),
                op: "Eq".into(),
                value: Value::String("arxiv".into()),
            }],
            projections: vec!["fact_id".into(), "content".into()],
            order_by: vec![],
            limit: None,
            offset: None,
            distinct: false,
            aggregate_count: false,
            with_ctes: vec![],
            group_by: vec![],
            aggregates: vec![],
            window_funcs: vec![],
            json_projections: vec![],
            json_filters: vec![],
            vector_filters: vec![],
            vector_score: None,
        });

        let mut q = base_query("Fact");
        q.with_ctes = vec![CteDef {
            alias: "arxiv_facts".into(),
            subquery: sub,
        }];
        q.projections = vec!["fact_id".into(), "content".into()];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "WITH arxiv_facts AS (SELECT fact_id, content FROM facts_view WHERE origin = 'arxiv') SELECT fact_id, content FROM facts_view"
        );
    }

    #[test]
    fn test_cte_multiple() {
        let arxiv_sub = Box::new(ColdQuery {
            label: "Fact".into(),
            filters: vec![ColdFilter {
                field: "origin".into(),
                op: "Eq".into(),
                value: Value::String("arxiv".into()),
            }],
            projections: vec!["fact_id".into(), "content".into()],
            order_by: vec![],
            limit: None,
            offset: None,
            distinct: false,
            aggregate_count: false,
            with_ctes: vec![],
            group_by: vec![],
            aggregates: vec![],
            window_funcs: vec![],
            json_projections: vec![],
            json_filters: vec![],
            vector_filters: vec![],
            vector_score: None,
        });

        let recent_sub = Box::new(ColdQuery {
            label: "Fact".into(),
            filters: vec![],
            projections: vec!["fact_id".into()],
            order_by: vec![ColdOrder {
                field: "created_at".into(),
                desc: true,
            }],
            limit: Some(100),
            offset: None,
            distinct: false,
            aggregate_count: false,
            with_ctes: vec![],
            group_by: vec![],
            aggregates: vec![],
            window_funcs: vec![],
            json_projections: vec![],
            json_filters: vec![],
            vector_filters: vec![],
            vector_score: None,
        });

        let mut q = base_query("Fact");
        q.with_ctes = vec![
            CteDef {
                alias: "arxiv_facts".into(),
                subquery: arxiv_sub,
            },
            CteDef {
                alias: "recent_facts".into(),
                subquery: recent_sub,
            },
        ];
        q.projections = vec!["fact_id".into()];
        let sql = translate(&q).unwrap();
        assert!(sql.starts_with("WITH arxiv_facts AS (SELECT fact_id, content FROM facts_view WHERE origin = 'arxiv'), recent_facts AS (SELECT fact_id FROM facts_view ORDER BY created_at DESC LIMIT 100)"));
        assert!(sql.ends_with("SELECT fact_id FROM facts_view"));
    }

    #[test]
    fn test_cte_nested_subquery_with_aggregate() {
        let sub = Box::new(ColdQuery {
            label: "Fact".into(),
            filters: vec![],
            projections: vec!["origin".into()],
            order_by: vec![],
            limit: None,
            offset: None,
            distinct: false,
            aggregate_count: false,
            with_ctes: vec![],
            group_by: vec!["origin".into()],
            aggregates: vec![AggregateDef {
                func: "COUNT".into(),
                column: "fact_id".into(),
                alias: Some("cnt".into()),
            }],
            window_funcs: vec![],
            json_projections: vec![],
            json_filters: vec![],
            vector_filters: vec![],
            vector_score: None,
        });

        let mut q = base_query("Fact");
        q.with_ctes = vec![CteDef {
            alias: "origin_counts".into(),
            subquery: sub,
        }];
        q.projections = vec!["origin".into(), "cnt".into()];
        q.order_by = vec![ColdOrder {
            field: "cnt".into(),
            desc: true,
        }];
        q.limit = Some(5);
        let sql = translate(&q).unwrap();
        assert!(sql.contains("WITH origin_counts AS (SELECT origin, COUNT(fact_id) AS cnt FROM facts_view GROUP BY origin)"));
        assert!(sql.contains("ORDER BY cnt DESC LIMIT 5"));
    }

    // ── Combined scenarios ────────────────────────────────────────────────

    #[test]
    fn test_combined_json_projection_with_group_by() {
        let mut q = base_query("Fact");
        q.projections = vec!["origin".into()];
        q.json_projections = vec![JsonProjection {
            column: "metadata".into(),
            path: "$.category".into(),
            alias: Some("category".into()),
        }];
        q.group_by = vec!["origin".into(), "category".into()];
        q.aggregates = vec![AggregateDef {
            func: "COUNT".into(),
            column: "fact_id".into(),
            alias: Some("cnt".into()),
        }];
        q.order_by = vec![ColdOrder {
            field: "cnt".into(),
            desc: true,
        }];
        q.limit = Some(10);
        let sql = translate(&q).unwrap();
        assert!(sql.contains("json_extract_string(metadata, '$.category') AS category"));
        assert!(sql.contains("GROUP BY origin, category"));
        assert!(sql.contains("ORDER BY cnt DESC LIMIT 10"));
    }

    #[test]
    fn test_combined_vector_filter_with_json_filter() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into()];
        q.filters = vec![ColdFilter {
            field: "origin".into(),
            op: "Eq".into(),
            value: Value::String("arxiv".into()),
        }];
        q.json_filters = vec![JsonFilter {
            column: "metadata".into(),
            path: "$.domain".into(),
            op: "Eq".into(),
            value: Value::String("cs.AI".into()),
        }];
        q.vector_filters = vec![VectorFilter {
            column: "embedding".into(),
            metric: "cosine".into(),
            vector: vec![0.1, 0.2, 0.3],
            op: "Gte".into(),
            threshold: 0.85,
        }];
        let sql = translate(&q).unwrap();
        assert!(sql.contains("origin = 'arxiv'"));
        assert!(sql.contains("json_extract_string(metadata, '$.domain') = 'cs.AI'"));
        assert!(sql.contains("array_cosine_similarity(embedding, [0.1, 0.2, 0.3]) >= 0.85"));
    }

    #[test]
    fn test_combined_window_with_vector_score() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into(), "origin".into(), "score".into()];
        q.window_funcs = vec![WindowFuncDef {
            func: "ROW_NUMBER".into(),
            column: None,
            partition_by: vec!["origin".into()],
            order_by: vec![ColdOrder {
                field: "score".into(),
                desc: true,
            }],
            alias: Some("rn".into()),
        }];
        q.vector_score = Some(VectorScore {
            column: "embedding".into(),
            metric: "cosine".into(),
            vector: vec![0.1, 0.2, 0.3],
            alias: None,
            sort_by_score: true,
        });
        q.limit = Some(20);
        let sql = translate(&q).unwrap();
        assert!(sql.contains("ROW_NUMBER() OVER (PARTITION BY origin ORDER BY score DESC) AS rn"));
        assert!(sql.contains("array_cosine_similarity(embedding, [0.1, 0.2, 0.3]) AS score"));
        // Two ORDER BYs: external (vector_score) overrides nothing, so both appear
        assert!(sql.contains("ORDER BY score DESC"));
        assert!(sql.contains("LIMIT 20"));
    }

    #[test]
    fn test_combined_fts_with_aggregate() {
        let mut q = base_query("Fact");
        q.projections = vec!["origin".into()];
        q.filters = vec![ColdFilter {
            field: "content".into(),
            op: "FtsMatch".into(),
            value: Value::String("deep learning".into()),
        }];
        q.group_by = vec!["origin".into()];
        q.aggregates = vec![AggregateDef {
            func: "COUNT".into(),
            column: "fact_id".into(),
            alias: Some("cnt".into()),
        }];
        q.order_by = vec![ColdOrder {
            field: "cnt".into(),
            desc: true,
        }];
        let sql = translate(&q).unwrap();
        assert!(sql.contains("fts_main_facts.match('deep learning')"));
        assert!(sql.contains("GROUP BY origin"));
        assert!(sql.contains("ORDER BY cnt DESC"));
    }

    // ── Edge cases ────────────────────────────────────────────────────────

    #[test]
    fn test_empty_group_by_with_aggregates() {
        // Aggregates without GROUP BY: valid SQL, single-row result
        let mut q = base_query("Fact");
        q.aggregates = vec![AggregateDef {
            func: "COUNT".into(),
            column: "fact_id".into(),
            alias: Some("total".into()),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(sql, "SELECT COUNT(fact_id) AS total FROM facts_view");
    }

    #[test]
    fn test_group_by_without_projections() {
        let mut q = base_query("Fact");
        q.group_by = vec!["origin".into()];
        q.aggregates = vec![AggregateDef {
            func: "COUNT".into(),
            column: "*".into(),
            alias: Some("cnt".into()),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT COUNT(*) AS cnt FROM facts_view GROUP BY origin"
        );
    }

    #[test]
    fn test_vec_f64_edge_cases() {
        // Integer-like floats should format as "{n}.0"
        let mut q = base_query("Fact");
        q.vector_filters = vec![VectorFilter {
            column: "v".into(),
            metric: "cosine".into(),
            vector: vec![0.0, 1.0, -1.0, 3.0],
            op: "Gte".into(),
            threshold: 0.5,
        }];
        let sql = translate(&q).unwrap();
        assert!(sql.contains("[0.0, 1.0, -1.0, 3.0]"));
    }

    #[test]
    fn test_value_to_sql_object() {
        let obj = serde_json::json!({"key": "value", "num": 42});
        let sql = value_to_sql(&obj);
        assert_eq!(sql, "'{\"key\":\"value\",\"num\":42}'");
    }

    #[test]
    fn test_quote_ident_reserved_word() {
        assert_eq!(quote_ident("select"), "select"); // unreserved in DuckDB context
        assert_eq!(quote_ident("count(fact_id)"), r#""count(fact_id)""#);
        assert_eq!(quote_ident("simple"), "simple");
        // Dotted qualified names: each segment quoted independently if needed.
        assert_eq!(quote_ident("a.b"), "a.b"); // simple: no quotes needed
        assert_eq!(quote_ident("a.b-c"), r#"a."b-c""#); // segment with hyphen needs quoting
        assert_eq!(quote_ident("tbl.col"), "tbl.col");
        assert_eq!(quote_ident("schema.table.col"), "schema.table.col");
    }

    #[test]
    fn test_extract_terms() {
        assert_eq!(extract_terms("'hello world'"), vec!["hello", "world"]);
        assert_eq!(extract_terms("hello"), vec!["hello"]);
        let empty: Vec<String> = vec![];
        assert_eq!(extract_terms(""), empty);
        assert_eq!(extract_terms("'a b c'"), vec!["a", "b", "c"]);
    }

    #[test]
    fn test_fts_index_name() {
        assert_eq!(fts_index_name("facts_view"), "fts_main_facts");
        assert_eq!(fts_index_name("intents_view"), "fts_main_intents");
        assert_eq!(fts_index_name("hints_view"), "fts_main_hints");
        assert_eq!(fts_index_name("custom_table"), "fts_main_custom_table");
    }

    // ── Gap coverage: untagged filter operators (Gt, Lt, Gte, Lte) ────────

    #[test]
    fn test_filter_gt() {
        let mut q = base_query("Fact");
        q.filters = vec![ColdFilter {
            field: "score".into(),
            op: "Gt".into(),
            value: Value::Number(serde_json::Number::from(80)),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(sql, "SELECT * FROM facts_view WHERE score > 80");
    }

    #[test]
    fn test_filter_lt() {
        let mut q = base_query("Fact");
        q.filters = vec![ColdFilter {
            field: "score".into(),
            op: "Lt".into(),
            value: Value::Number(serde_json::Number::from(50)),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(sql, "SELECT * FROM facts_view WHERE score < 50");
    }

    #[test]
    fn test_filter_gte() {
        let mut q = base_query("Fact");
        q.filters = vec![ColdFilter {
            field: "score".into(),
            op: "Gte".into(),
            value: Value::Number(serde_json::Number::from(60)),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(sql, "SELECT * FROM facts_view WHERE score >= 60");
    }

    #[test]
    fn test_filter_lte() {
        let mut q = base_query("Fact");
        q.filters = vec![ColdFilter {
            field: "score".into(),
            op: "Lte".into(),
            value: Value::Number(serde_json::Number::from(100)),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(sql, "SELECT * FROM facts_view WHERE score <= 100");
    }

    #[test]
    fn test_filter_in_non_array_error() {
        let mut q = base_query("Fact");
        q.filters = vec![ColdFilter {
            field: "origin".into(),
            op: "In".into(),
            value: Value::String("not_an_array".into()),
        }];
        let result = translate(&q);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("In filter requires an array value")
        );
    }

    // ── Gap coverage: JSON filter Ne, Lt, Gte, Lte ────────────────────────

    #[test]
    fn test_json_filter_ne() {
        let mut q = base_query("Fact");
        q.json_filters = vec![JsonFilter {
            column: "metadata".into(),
            path: "$.status".into(),
            op: "Ne".into(),
            value: Value::String("archived".into()),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM facts_view WHERE json_extract_string(metadata, '$.status') != 'archived'"
        );
    }

    #[test]
    fn test_json_filter_lt() {
        let mut q = base_query("Fact");
        q.json_filters = vec![JsonFilter {
            column: "metadata".into(),
            path: "$.priority".into(),
            op: "Lt".into(),
            value: Value::Number(serde_json::Number::from(3)),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM facts_view WHERE json_extract_string(metadata, '$.priority') < 3"
        );
    }

    #[test]
    fn test_json_filter_gte() {
        let mut q = base_query("Fact");
        q.json_filters = vec![JsonFilter {
            column: "metadata".into(),
            path: "$.confidence".into(),
            op: "Gte".into(),
            value: Value::Number(serde_json::Number::from(90)),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM facts_view WHERE json_extract_string(metadata, '$.confidence') >= 90"
        );
    }

    #[test]
    fn test_json_filter_lte() {
        let mut q = base_query("Fact");
        q.json_filters = vec![JsonFilter {
            column: "metadata".into(),
            path: "$.revision".into(),
            op: "Lte".into(),
            value: Value::Number(serde_json::Number::from(5)),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM facts_view WHERE json_extract_string(metadata, '$.revision') <= 5"
        );
    }

    #[test]
    fn test_json_filter_in_non_array_error() {
        let mut q = base_query("Fact");
        q.json_filters = vec![JsonFilter {
            column: "metadata".into(),
            path: "$.category".into(),
            op: "In".into(),
            value: Value::String("not_an_array".into()),
        }];
        let result = translate(&q);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .contains("In filter requires an array value")
        );
    }

    // ── Gap coverage: FtsMatchAnd (semantically distinct from FtsMatch) ────

    #[test]
    fn test_fts_match_and_multi_term() {
        // FtsMatchAnd generates AND of CONTAINS per term (no FTS index needed).
        let mut q = base_query("Fact");
        q.filters = vec![ColdFilter {
            field: "content".into(),
            op: "FtsMatchAnd".into(),
            value: Value::String("neural network transformer".into()),
        }];
        let sql = translate(&q).unwrap();
        assert!(sql.contains("CONTAINS(content, 'neural') AND CONTAINS(content, 'network') AND CONTAINS(content, 'transformer')"));
        assert!(!sql.contains("fts_main"));
    }

    #[test]
    fn test_fts_match_and_single_term_falls_back() {
        let mut q = base_query("Fact");
        q.filters = vec![ColdFilter {
            field: "content".into(),
            op: "FtsMatchAnd".into(),
            value: Value::String("single".into()),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM facts_view WHERE CONTAINS(content, 'single')"
        );
    }

    // ── Gap coverage: offset_without_limit ─────────────────────────────────

    #[test]
    fn test_offset_without_limit() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into()];
        q.offset = Some(10);
        let sql = translate(&q).unwrap();
        // DuckDB requires LIMIT with OFFSET; translator emits LIMIT 1000000
        assert_eq!(
            sql,
            "SELECT fact_id FROM facts_view LIMIT 1000000 OFFSET 10"
        );
    }

    // ── Gap coverage: SELECT DISTINCT * ────────────────────────────────────

    #[test]
    fn test_select_distinct_star() {
        let mut q = base_query("Fact");
        q.distinct = true;
        // Empty projections + distinct = SELECT DISTINCT *
        let sql = translate(&q).unwrap();
        assert_eq!(sql, "SELECT DISTINCT * FROM facts_view");
    }

    // ── Gap coverage: window function unknown func pass-through ────────────

    #[test]
    fn test_window_unknown_func_passthrough() {
        let mut q = base_query("Fact");
        q.projections = vec!["fact_id".into()];
        q.window_funcs = vec![WindowFuncDef {
            func: "NTILE".into(),
            column: Some("score".into()),
            partition_by: vec!["origin".into()],
            order_by: vec![],
            alias: Some("quartile".into()),
        }];
        let sql = translate(&q).unwrap();
        // Unknown func names pass through as-is: NTILE(score)
        assert!(sql.contains("NTILE(score) OVER (PARTITION BY origin) AS quartile"));
    }

    // ── Gap coverage: CTE subquery error propagation ───────────────────────

    #[test]
    fn test_cte_subquery_error_propagation() {
        let bad_sub = Box::new(ColdQuery {
            label: "UnknownLabel".into(),
            filters: vec![],
            projections: vec![],
            order_by: vec![],
            limit: None,
            offset: None,
            distinct: false,
            aggregate_count: false,
            with_ctes: vec![],
            group_by: vec![],
            aggregates: vec![],
            window_funcs: vec![],
            json_projections: vec![],
            json_filters: vec![],
            vector_filters: vec![],
            vector_score: None,
        });

        let mut q = base_query("Fact");
        q.with_ctes = vec![CteDef {
            alias: "bad_cte".into(),
            subquery: bad_sub,
        }];
        let result = translate(&q);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown label"));
    }

    // ── Gap coverage: FTS on Hint table ───────────────────────────────────

    #[test]
    fn test_fts_match_hint() {
        let mut q = base_query("Hint");
        q.projections = vec!["hint_id".into()];
        q.filters = vec![ColdFilter {
            field: "content".into(),
            op: "FtsMatch".into(),
            value: Value::String("action item".into()),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT hint_id FROM hints_view WHERE content IN (SELECT doc_id FROM fts_main_hints WHERE fts_main_hints.match('action item'))"
        );
    }

    // ── Gap coverage: dotted column in GROUP BY ───────────────────────────

    #[test]
    fn test_group_by_dotted_column() {
        // quote_ident splits on dot: each segment is quoted independently.
        // Simple segments like "f", "origin", "fact_id" need no quoting.
        let mut q = base_query("Fact");
        q.projections = vec!["f.origin".into()];
        q.group_by = vec!["f.origin".into()];
        q.aggregates = vec![AggregateDef {
            func: "COUNT".into(),
            column: "f.fact_id".into(),
            alias: Some("cnt".into()),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT f.origin, COUNT(f.fact_id) AS cnt FROM facts_view GROUP BY f.origin"
        );
    }

    // ── Gap coverage: distinct + aggregates ───────────────────────────────

    #[test]
    fn test_distinct_with_aggregates() {
        let mut q = base_query("Fact");
        q.projections = vec!["origin".into()];
        q.distinct = true;
        q.aggregates = vec![AggregateDef {
            func: "COUNT".into(),
            column: "fact_id".into(),
            alias: Some("cnt".into()),
        }];
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT DISTINCT origin, COUNT(fact_id) AS cnt FROM facts_view"
        );
    }
}
