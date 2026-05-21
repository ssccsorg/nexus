// DuckDB Cypher→SQL translator.
//
// Translates a simple intermediate query description (ColdQuery) into a DuckDB
// SQL SELECT statement against parquet-backed FIH views (facts_view,
// intents_view, hints_view).
//
// ColdQuery is a JSON-serializable struct that the graph layer's Cypher executor
// produces when it determines a query cannot benefit from graph traversal and
// should be routed to cold storage.
//
// This is more limited than the full cyrs_plan::ReadOp pipeline:
//   - Only tabular scans (Fact, Intent, Hint) — no graph expansion
//   - Property filters on known columns
//   - Projections, ordering, limit/offset
//   - Simple COUNT aggregation
//
// Future work (#51+, #35) may add direct ReadOp→SQL translation.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A simple tabular query for DuckDB cold storage.
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
}

/// A single filter condition (AND-composed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColdFilter {
    /// Column name (e.g. "origin", "creator", "fact_id").
    pub field: String,

    /// Operator: "Eq", "Ne", "Gt", "Lt", "Gte", "Lte", "In", "Contains".
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

/// Translate a ColdQuery into a DuckDB SQL SELECT string.
pub fn translate(query: &ColdQuery) -> Result<String, String> {
    let table = match query.label.as_str() {
        "Fact" => "facts_view",
        "Intent" => "intents_view",
        "Hint" => "hints_view",
        other => {
            return Err(format!(
                "unknown label '{other}': expected Fact, Intent, or Hint"
            ));
        }
    };

    // Build SQL: all SELECT cases already include FROM {table}
    // (except aggregate_count which doesn't need a table reference).
    let sql = if query.aggregate_count {
        "SELECT COUNT(*) as count".to_string()
    } else if query.projections.is_empty() {
        format!("SELECT * FROM {table}")
    } else if query.distinct {
        let cols = query.projections.join(", ");
        format!("SELECT DISTINCT {cols} FROM {table}")
    } else {
        let cols = query.projections.join(", ");
        format!("SELECT {cols} FROM {table}")
    };

    // Build WHERE clause
    let mut where_clauses: Vec<String> = Vec::new();
    for f in &query.filters {
        let clause = translate_filter(f)?;
        where_clauses.push(clause);
    }

    let mut sql = if where_clauses.is_empty() {
        sql
    } else {
        format!("{sql} WHERE {}", where_clauses.join(" AND "))
    };

    // Build ORDER BY
    if !query.order_by.is_empty() && !query.aggregate_count {
        let order_parts: Vec<String> = query
            .order_by
            .iter()
            .map(|o| {
                let dir = if o.desc { "DESC" } else { "ASC" };
                format!("{} {}", o.field, dir)
            })
            .collect();
        sql = format!("{sql} ORDER BY {}", order_parts.join(", "));
    }

    // Build LIMIT / OFFSET
    match (query.limit, query.offset) {
        (Some(limit), Some(offset)) => {
            sql = format!("{sql} LIMIT {} OFFSET {}", limit, offset);
        }
        (Some(limit), None) => {
            sql = format!("{sql} LIMIT {}", limit);
        }
        (None, Some(offset)) => {
            // DuckDB requires LIMIT with OFFSET; use a large limit
            sql = format!("{sql} LIMIT 1000000 OFFSET {}", offset);
        }
        (None, None) => {}
    }

    Ok(sql)
}

/// Translate a single filter condition to a SQL WHERE fragment.
fn translate_filter(f: &ColdFilter) -> Result<String, String> {
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
        "Contains" => {
            // DuckDB supports CONTAINS / strpos
            Ok(format!("CONTAINS({col}, {val_str})"))
        }
        other => Err(format!("unknown filter operator '{other}'")),
    }
}

/// Format a serde_json::Value as a DuckDB SQL literal.
fn value_to_sql(v: &Value) -> String {
    match v {
        Value::String(s) => {
            // Escape single quotes by doubling them
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
            // JSON objects as strings (for DuckDB JSON columns)
            let s = serde_json::to_string(v).unwrap_or_default();
            let escaped = s.replace('\'', "''");
            format!("'{}'", escaped)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_translate_fact_scan() {
        let q = ColdQuery {
            label: "Fact".into(),
            filters: vec![],
            projections: vec!["fact_id".into(), "origin".into()],
            order_by: vec![],
            limit: None,
            offset: None,
            distinct: false,
            aggregate_count: false,
        };
        let sql = translate(&q).unwrap();
        assert_eq!(sql, "SELECT fact_id, origin FROM facts_view");
    }

    #[test]
    fn test_translate_intent_scan() {
        let q = ColdQuery {
            label: "Intent".into(),
            filters: vec![],
            projections: vec!["intent_id".into(), "description".into()],
            order_by: vec![],
            limit: None,
            offset: None,
            distinct: false,
            aggregate_count: false,
        };
        let sql = translate(&q).unwrap();
        assert_eq!(sql, "SELECT intent_id, description FROM intents_view");
    }

    #[test]
    fn test_translate_hint_scan() {
        let q = ColdQuery {
            label: "Hint".into(),
            filters: vec![],
            projections: vec!["hint_id".into(), "content".into()],
            order_by: vec![],
            limit: None,
            offset: None,
            distinct: false,
            aggregate_count: false,
        };
        let sql = translate(&q).unwrap();
        assert_eq!(sql, "SELECT hint_id, content FROM hints_view");
    }

    #[test]
    fn test_translate_all_columns() {
        let q = ColdQuery {
            label: "Fact".into(),
            filters: vec![],
            projections: vec![],
            order_by: vec![],
            limit: None,
            offset: None,
            distinct: false,
            aggregate_count: false,
        };
        let sql = translate(&q).unwrap();
        assert_eq!(sql, "SELECT * FROM facts_view");
    }

    #[test]
    fn test_translate_filter_eq() {
        let q = ColdQuery {
            label: "Fact".into(),
            filters: vec![ColdFilter {
                field: "origin".into(),
                op: "Eq".into(),
                value: Value::String("arxiv_2401".into()),
            }],
            projections: vec!["fact_id".into()],
            order_by: vec![],
            limit: None,
            offset: None,
            distinct: false,
            aggregate_count: false,
        };
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT fact_id FROM facts_view WHERE origin = 'arxiv_2401'"
        );
    }

    #[test]
    fn test_translate_filter_multiple() {
        let q = ColdQuery {
            label: "Fact".into(),
            filters: vec![
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
            ],
            projections: vec!["fact_id".into()],
            order_by: vec![],
            limit: None,
            offset: None,
            distinct: false,
            aggregate_count: false,
        };
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT fact_id FROM facts_view WHERE origin = 'test' AND creator = 'agent-a'"
        );
    }

    #[test]
    fn test_translate_order_limit_offset() {
        let q = ColdQuery {
            label: "Fact".into(),
            filters: vec![],
            projections: vec!["fact_id".into()],
            order_by: vec![ColdOrder {
                field: "created_at".into(),
                desc: true,
            }],
            limit: Some(10),
            offset: Some(5),
            distinct: false,
            aggregate_count: false,
        };
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT fact_id FROM facts_view ORDER BY created_at DESC LIMIT 10 OFFSET 5"
        );
    }

    #[test]
    fn test_translate_distinct() {
        let q = ColdQuery {
            label: "Fact".into(),
            filters: vec![],
            projections: vec!["origin".into()],
            order_by: vec![],
            limit: None,
            offset: None,
            distinct: true,
            aggregate_count: false,
        };
        let sql = translate(&q).unwrap();
        assert_eq!(sql, "SELECT DISTINCT origin FROM facts_view");
    }

    #[test]
    fn test_translate_count() {
        let q = ColdQuery {
            label: "Fact".into(),
            filters: vec![],
            projections: vec![],
            order_by: vec![],
            limit: None,
            offset: None,
            distinct: false,
            aggregate_count: true,
        };
        let sql = translate(&q).unwrap();
        assert_eq!(sql, "SELECT COUNT(*) as count");
    }

    #[test]
    fn test_translate_in_filter() {
        let q = ColdQuery {
            label: "Fact".into(),
            filters: vec![ColdFilter {
                field: "fact_id".into(),
                op: "In".into(),
                value: Value::Array(vec![
                    Value::String("f001".into()),
                    Value::String("f002".into()),
                ]),
            }],
            projections: vec!["fact_id".into()],
            order_by: vec![],
            limit: None,
            offset: None,
            distinct: false,
            aggregate_count: false,
        };
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
        };
        let result = translate(&q);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown label"));
    }

    #[test]
    fn test_translate_contains_filter() {
        let q = ColdQuery {
            label: "Fact".into(),
            filters: vec![ColdFilter {
                field: "content".into(),
                op: "Contains".into(),
                value: Value::String("neural".into()),
            }],
            projections: vec!["fact_id".into()],
            order_by: vec![],
            limit: None,
            offset: None,
            distinct: false,
            aggregate_count: false,
        };
        let sql = translate(&q).unwrap();
        assert_eq!(
            sql,
            "SELECT fact_id FROM facts_view WHERE CONTAINS(content, 'neural')"
        );
    }

    #[test]
    fn test_translate_unknown_operator() {
        let q = ColdQuery {
            label: "Fact".into(),
            filters: vec![ColdFilter {
                field: "origin".into(),
                op: "Regex".into(),
                value: Value::String(".*".into()),
            }],
            projections: vec!["fact_id".into()],
            order_by: vec![],
            limit: None,
            offset: None,
            distinct: false,
            aggregate_count: false,
        };
        let result = translate(&q);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown filter operator"));
    }
}
