use interface_query::{AggregateDef, ColdFilter, ColdOrder, ColdQuery};
use nexus_storage_duckdb::cypher_sql::*;
use nexus_storage_duckdb::duckdb_ext::{
    CteDef, DuckDbQueryExt, JsonFilter, JsonProjection, VectorFilter, VectorScore, WindowFuncDef,
};
use serde_json::Value;

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
        group_by: vec![],
        aggregates: vec![],
    }
}

// ── Existing tests (unchanged semantics) ───────────────────────────────

#[test]
fn test_translate_fact_scan() {
    let mut q = base_query("Fact");
    q.projections = vec!["fact_id".into(), "origin".into()];
    let sql = translate(&q, None).unwrap();
    assert_eq!(sql, "SELECT fact_id, origin FROM facts_view");
}

#[test]
fn test_translate_intent_scan() {
    let mut q = base_query("Intent");
    q.projections = vec!["intent_id".into(), "description".into()];
    let sql = translate(&q, None).unwrap();
    assert_eq!(sql, "SELECT intent_id, description FROM intents_view");
}

#[test]
fn test_translate_hint_scan() {
    let mut q = base_query("Hint");
    q.projections = vec!["hint_id".into(), "content".into()];
    let sql = translate(&q, None).unwrap();
    assert_eq!(sql, "SELECT hint_id, content FROM hints_view");
}

#[test]
fn test_translate_all_columns() {
    let q = base_query("Fact");
    let sql = translate(&q, None).unwrap();
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
    let sql = translate(&q, None).unwrap();
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
    let sql = translate(&q, None).unwrap();
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
    let sql = translate(&q, None).unwrap();
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
    let sql = translate(&q, None).unwrap();
    assert_eq!(sql, "SELECT DISTINCT origin FROM facts_view");
}

#[test]
fn test_translate_count() {
    let mut q = base_query("Fact");
    q.aggregate_count = true;
    let sql = translate(&q, None).unwrap();
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
    let sql = translate(&q, None).unwrap();
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
        group_by: vec![],
        aggregates: vec![],
    };
    let result = translate(&q, None);
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
    let sql = translate(&q, None).unwrap();
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
    let result = translate(&q, None);
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
    let sql = translate(&q, None).unwrap();
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
    let sql = translate(&q, None).unwrap();
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
    let sql = translate(&q, None).unwrap();
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
    let sql = translate(&q, None).unwrap();
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
    let sql = translate(&q, None).unwrap();
    assert!(sql.contains("origin = 'arxiv'"));
    assert!(sql.contains(
        "content IN (SELECT doc_id FROM fts_main_facts WHERE fts_main_facts.match('deep learning'))"
    ));
    assert!(sql.contains("AND"));
}

// ── Vector similarity filters ─────────────────────────────────────────

#[test]
fn test_vector_cosine_similarity_filter() {
    let mut q = base_query("Fact");
    q.projections = vec!["fact_id".into()];
    let mut ext = DuckDbQueryExt::default();
    ext.vector_filters = vec![VectorFilter {
        column: "embedding".into(),
        metric: "cosine".into(),
        vector: vec![0.1, 0.2, 0.3],
        op: "Gte".into(),
        threshold: 0.8,
    }];
    let sql = translate(&q, Some(&ext)).unwrap();
    assert_eq!(
        sql,
        "SELECT fact_id FROM facts_view WHERE array_cosine_similarity(embedding, [0.1, 0.2, 0.3]) >= 0.8"
    );
}

#[test]
fn test_vector_euclidean_distance_filter() {
    let mut q = base_query("Fact");
    q.projections = vec!["fact_id".into()];
    let mut ext = DuckDbQueryExt::default();
    ext.vector_filters = vec![VectorFilter {
        column: "embedding".into(),
        metric: "euclidean".into(),
        vector: vec![1.0, 2.0, 3.0],
        op: "Lte".into(),
        threshold: 5.0,
    }];
    let sql = translate(&q, Some(&ext)).unwrap();
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
    let mut ext = DuckDbQueryExt::default();
    ext.vector_filters = vec![VectorFilter {
        column: "embedding".into(),
        metric: "cosine".into(),
        vector: vec![0.5, 0.5],
        op: "Gte".into(),
        threshold: 0.9,
    }];
    let sql = translate(&q, Some(&ext)).unwrap();
    assert!(sql.contains("origin = 'arxiv'"));
    assert!(sql.contains("array_cosine_similarity(embedding, [0.5, 0.5]) >= 0.9"));
    assert!(sql.contains("AND"));
}

#[test]
fn test_vector_filter_unknown_metric() {
    let q = base_query("Fact");
    let mut ext = DuckDbQueryExt::default();
    ext.vector_filters = vec![VectorFilter {
        column: "v".into(),
        metric: "manhattan".into(),
        vector: vec![1.0],
        op: "Gte".into(),
        threshold: 0.5,
    }];
    let result = translate(&q, Some(&ext));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("unknown vector metric"));
}

#[test]
fn test_vector_filter_unknown_operator() {
    let q = base_query("Fact");
    let mut ext = DuckDbQueryExt::default();
    ext.vector_filters = vec![VectorFilter {
        column: "v".into(),
        metric: "cosine".into(),
        vector: vec![1.0],
        op: "Eq".into(),
        threshold: 0.5,
    }];
    let result = translate(&q, Some(&ext));
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
    let mut ext = DuckDbQueryExt::default();
    ext.vector_score = Some(VectorScore {
        column: "embedding".into(),
        metric: "cosine".into(),
        vector: vec![0.1, 0.2, 0.3],
        alias: Some("similarity".into()),
        sort_by_score: true,
    });
    let sql = translate(&q, Some(&ext)).unwrap();
    assert_eq!(
        sql,
        "SELECT fact_id, array_cosine_similarity(embedding, [0.1, 0.2, 0.3]) AS similarity FROM facts_view ORDER BY similarity DESC"
    );
}

#[test]
fn test_vector_score_euclidean() {
    let q = base_query("Fact");
    let mut ext = DuckDbQueryExt::default();
    ext.vector_score = Some(VectorScore {
        column: "embedding".into(),
        metric: "euclidean".into(),
        vector: vec![1.0, 2.0, 3.0],
        alias: None,
        sort_by_score: true,
    });
    let sql = translate(&q, Some(&ext)).unwrap();
    assert_eq!(
        sql,
        "SELECT *, array_distance(embedding, [1.0, 2.0, 3.0]) AS score FROM facts_view ORDER BY score ASC"
    );
}

#[test]
fn test_vector_score_without_sort() {
    let mut q = base_query("Fact");
    q.projections = vec!["fact_id".into()];
    let mut ext = DuckDbQueryExt::default();
    ext.vector_score = Some(VectorScore {
        column: "embedding".into(),
        metric: "cosine".into(),
        vector: vec![0.1, 0.2, 0.3],
        alias: None,
        sort_by_score: false,
    });
    let sql = translate(&q, Some(&ext)).unwrap();
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
    let mut ext = DuckDbQueryExt::default();
    ext.vector_score = Some(VectorScore {
        column: "embedding".into(),
        metric: "cosine".into(),
        vector: vec![0.1, 0.2, 0.3],
        alias: None,
        sort_by_score: true,
    });
    let sql = translate(&q, Some(&ext)).unwrap();
    // Both ORDER BY clauses present
    assert!(sql.contains("ORDER BY created_at DESC, score DESC"));
    assert!(sql.contains("array_cosine_similarity(embedding, [0.1, 0.2, 0.3]) AS score"));
}

#[test]
fn test_vector_score_unknown_metric() {
    let q = base_query("Fact");
    let mut ext = DuckDbQueryExt::default();
    ext.vector_score = Some(VectorScore {
        column: "embedding".into(),
        metric: "dot_product".into(),
        vector: vec![1.0],
        alias: None,
        sort_by_score: false,
    });
    let result = translate(&q, Some(&ext));
    assert!(result.is_err());
}

// ── JSON projections ──────────────────────────────────────────────────

#[test]
fn test_json_extract_projection() {
    let mut q = base_query("Fact");
    q.projections = vec!["fact_id".into()];
    let mut ext = DuckDbQueryExt::default();
    ext.json_projections = vec![JsonProjection {
        column: "metadata".into(),
        path: "$.category".into(),
        alias: Some("category".into()),
    }];
    let sql = translate(&q, Some(&ext)).unwrap();
    assert_eq!(
        sql,
        "SELECT fact_id, json_extract_string(metadata, '$.category') AS category FROM facts_view"
    );
}

#[test]
fn test_json_extract_multiple() {
    let mut q = base_query("Fact");
    q.projections = vec!["fact_id".into()];
    let mut ext = DuckDbQueryExt::default();
    ext.json_projections = vec![
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
    let sql = translate(&q, Some(&ext)).unwrap();
    assert!(sql.contains("json_extract_string(metadata, '$.domain') AS domain"));
    assert!(sql.contains("json_extract_string(metadata, '$.version') AS version"));
}

#[test]
fn test_json_extract_only() {
    let q = base_query("Fact");
    let mut ext = DuckDbQueryExt::default();
    ext.json_projections = vec![JsonProjection {
        column: "metadata".into(),
        path: "category".into(),
        alias: None,
    }];
    let sql = translate(&q, Some(&ext)).unwrap();
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
    let mut ext = DuckDbQueryExt::default();
    ext.json_filters = vec![JsonFilter {
        column: "metadata".into(),
        path: "$.domain".into(),
        op: "Eq".into(),
        value: Value::String("mathematics".into()),
    }];
    let sql = translate(&q, Some(&ext)).unwrap();
    assert_eq!(
        sql,
        "SELECT fact_id FROM facts_view WHERE json_extract_string(metadata, '$.domain') = 'mathematics'"
    );
}

#[test]
fn test_json_filter_gt() {
    let q = base_query("Fact");
    let mut ext = DuckDbQueryExt::default();
    ext.json_filters = vec![JsonFilter {
        column: "metadata".into(),
        path: "$.score".into(),
        op: "Gt".into(),
        value: Value::Number(serde_json::Number::from(85)),
    }];
    let sql = translate(&q, Some(&ext)).unwrap();
    assert_eq!(
        sql,
        "SELECT * FROM facts_view WHERE json_extract_string(metadata, '$.score') > 85"
    );
}

#[test]
fn test_json_filter_contains() {
    let q = base_query("Fact");
    let mut ext = DuckDbQueryExt::default();
    ext.json_filters = vec![JsonFilter {
        column: "metadata".into(),
        path: "$.tags".into(),
        op: "Contains".into(),
        value: Value::String("graph".into()),
    }];
    let sql = translate(&q, Some(&ext)).unwrap();
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
    let mut ext = DuckDbQueryExt::default();
    ext.json_filters = vec![JsonFilter {
        column: "metadata".into(),
        path: "$.domain".into(),
        op: "Eq".into(),
        value: Value::String("cs.AI".into()),
    }];
    let sql = translate(&q, Some(&ext)).unwrap();
    assert!(sql.contains("origin = 'arxiv'"));
    assert!(sql.contains("json_extract_string(metadata, '$.domain') = 'cs.AI'"));
    assert!(sql.contains("AND"));
}

#[test]
fn test_json_filter_in() {
    let q = base_query("Fact");
    let mut ext = DuckDbQueryExt::default();
    ext.json_filters = vec![JsonFilter {
        column: "metadata".into(),
        path: "$.category".into(),
        op: "In".into(),
        value: Value::Array(vec![
            Value::String("math".into()),
            Value::String("physics".into()),
        ]),
    }];
    let sql = translate(&q, Some(&ext)).unwrap();
    assert_eq!(
        sql,
        "SELECT * FROM facts_view WHERE json_extract_string(metadata, '$.category') IN ('math', 'physics')"
    );
}

#[test]
fn test_json_filter_unknown_operator() {
    let q = base_query("Fact");
    let mut ext = DuckDbQueryExt::default();
    ext.json_filters = vec![JsonFilter {
        column: "metadata".into(),
        path: "$.x".into(),
        op: "Regex".into(),
        value: Value::String(".*".into()),
    }];
    let result = translate(&q, Some(&ext));
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
    let sql = translate(&q, None).unwrap();
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
    let sql = translate(&q, None).unwrap();
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
    let sql = translate(&q, None).unwrap();
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
    let sql = translate(&q, None).unwrap();
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
    let sql = translate(&q, None).unwrap();
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
    let sql = translate(&q, None).unwrap();
    // Default alias: "count(fact_id)"
    assert!(sql.contains("COUNT(fact_id) AS \"count(fact_id)\""));
}

// ── Window functions ──────────────────────────────────────────────────

#[test]
fn test_window_row_number() {
    let mut q = base_query("Fact");
    q.projections = vec!["fact_id".into(), "origin".into()];
    let mut ext = DuckDbQueryExt::default();
    ext.window_funcs = vec![WindowFuncDef {
        func: "ROW_NUMBER".into(),
        column: None,
        partition_by: vec!["origin".into()],
        order_by: vec![ColdOrder {
            field: "created_at".into(),
            desc: true,
        }],
        alias: Some("rn".into()),
    }];
    let sql = translate(&q, Some(&ext)).unwrap();
    assert_eq!(
        sql,
        "SELECT fact_id, origin, ROW_NUMBER() OVER (PARTITION BY origin ORDER BY created_at DESC) AS rn FROM facts_view"
    );
}

#[test]
fn test_window_rank() {
    let mut q = base_query("Fact");
    q.projections = vec!["fact_id".into()];
    let mut ext = DuckDbQueryExt::default();
    ext.window_funcs = vec![WindowFuncDef {
        func: "RANK".into(),
        column: None,
        partition_by: vec![],
        order_by: vec![ColdOrder {
            field: "score".into(),
            desc: true,
        }],
        alias: Some("rank".into()),
    }];
    let sql = translate(&q, Some(&ext)).unwrap();
    assert_eq!(
        sql,
        "SELECT fact_id, RANK() OVER (ORDER BY score DESC) AS rank FROM facts_view"
    );
}

#[test]
fn test_window_dense_rank() {
    let q = base_query("Fact");
    let mut ext = DuckDbQueryExt::default();
    ext.window_funcs = vec![WindowFuncDef {
        func: "DENSE_RANK".into(),
        column: None,
        partition_by: vec![],
        order_by: vec![],
        alias: None,
    }];
    let sql = translate(&q, Some(&ext)).unwrap();
    assert!(sql.contains("DENSE_RANK() OVER () AS dense_rank_window"));
}

#[test]
fn test_window_sum_partition() {
    let mut q = base_query("Fact");
    q.projections = vec!["origin".into(), "score".into()];
    let mut ext = DuckDbQueryExt::default();
    ext.window_funcs = vec![WindowFuncDef {
        func: "SUM".into(),
        column: Some("score".into()),
        partition_by: vec!["origin".into()],
        order_by: vec![],
        alias: Some("origin_total".into()),
    }];
    let sql = translate(&q, Some(&ext)).unwrap();
    assert_eq!(
        sql,
        "SELECT origin, score, SUM(score) OVER (PARTITION BY origin) AS origin_total FROM facts_view"
    );
}

#[test]
fn test_window_lead_lag() {
    let mut q = base_query("Fact");
    q.projections = vec!["fact_id".into(), "created_at".into()];
    let mut ext = DuckDbQueryExt::default();
    ext.window_funcs = vec![
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
    let sql = translate(&q, Some(&ext)).unwrap();
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
    let mut ext = DuckDbQueryExt::default();
    ext.window_funcs = vec![WindowFuncDef {
        func: "FIRST_VALUE".into(),
        column: Some("fact_id".into()),
        partition_by: vec!["origin".into()],
        order_by: vec![ColdOrder {
            field: "created_at".into(),
            desc: false,
        }],
        alias: Some("first_fact".into()),
    }];
    let sql = translate(&q, Some(&ext)).unwrap();
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
        group_by: vec![],
        aggregates: vec![],
    });

    let mut q = base_query("Fact");
    let mut ext = DuckDbQueryExt::default();
    ext.with_ctes = vec![CteDef {
        alias: "arxiv_facts".into(),
        subquery: sub,
    }];
    q.projections = vec!["fact_id".into(), "content".into()];
    let sql = translate(&q, Some(&ext)).unwrap();
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
        group_by: vec![],
        aggregates: vec![],
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
        group_by: vec![],
        aggregates: vec![],
    });

    let mut q = base_query("Fact");
    let mut ext = DuckDbQueryExt::default();
    ext.with_ctes = vec![
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
    let sql = translate(&q, Some(&ext)).unwrap();
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
        group_by: vec!["origin".into()],
        aggregates: vec![AggregateDef {
            func: "COUNT".into(),
            column: "fact_id".into(),
            alias: Some("cnt".into()),
        }],
    });

    let mut q = base_query("Fact");
    let mut ext = DuckDbQueryExt::default();
    ext.with_ctes = vec![CteDef {
        alias: "origin_counts".into(),
        subquery: sub,
    }];
    q.projections = vec!["origin".into(), "cnt".into()];
    q.order_by = vec![ColdOrder {
        field: "cnt".into(),
        desc: true,
    }];
    q.limit = Some(5);
    let sql = translate(&q, Some(&ext)).unwrap();
    assert!(sql.contains("WITH origin_counts AS (SELECT origin, COUNT(fact_id) AS cnt FROM facts_view GROUP BY origin)"));
    assert!(sql.contains("ORDER BY cnt DESC LIMIT 5"));
}

// ── Combined scenarios ────────────────────────────────────────────────

#[test]
fn test_combined_json_projection_with_group_by() {
    let mut q = base_query("Fact");
    q.projections = vec!["origin".into()];
    let mut ext = DuckDbQueryExt::default();
    ext.json_projections = vec![JsonProjection {
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
    let sql = translate(&q, Some(&ext)).unwrap();
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
    let mut ext = DuckDbQueryExt::default();
    ext.json_filters = vec![JsonFilter {
        column: "metadata".into(),
        path: "$.domain".into(),
        op: "Eq".into(),
        value: Value::String("cs.AI".into()),
    }];
    ext.vector_filters = vec![VectorFilter {
        column: "embedding".into(),
        metric: "cosine".into(),
        vector: vec![0.1, 0.2, 0.3],
        op: "Gte".into(),
        threshold: 0.85,
    }];
    let sql = translate(&q, Some(&ext)).unwrap();
    assert!(sql.contains("origin = 'arxiv'"));
    assert!(sql.contains("json_extract_string(metadata, '$.domain') = 'cs.AI'"));
    assert!(sql.contains("array_cosine_similarity(embedding, [0.1, 0.2, 0.3]) >= 0.85"));
}

#[test]
fn test_combined_window_with_vector_score() {
    let mut q = base_query("Fact");
    q.projections = vec!["fact_id".into(), "origin".into(), "score".into()];
    let mut ext = DuckDbQueryExt::default();
    ext.window_funcs = vec![WindowFuncDef {
        func: "ROW_NUMBER".into(),
        column: None,
        partition_by: vec!["origin".into()],
        order_by: vec![ColdOrder {
            field: "score".into(),
            desc: true,
        }],
        alias: Some("rn".into()),
    }];
    ext.vector_score = Some(VectorScore {
        column: "embedding".into(),
        metric: "cosine".into(),
        vector: vec![0.1, 0.2, 0.3],
        alias: None,
        sort_by_score: true,
    });
    q.limit = Some(20);
    let sql = translate(&q, Some(&ext)).unwrap();
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
    let sql = translate(&q, None).unwrap();
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
    let sql = translate(&q, None).unwrap();
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
    let sql = translate(&q, None).unwrap();
    assert_eq!(
        sql,
        "SELECT COUNT(*) AS cnt FROM facts_view GROUP BY origin"
    );
}

#[test]
fn test_vec_f64_edge_cases() {
    // Integer-like floats should format as "{n}.0"
    let q = base_query("Fact");
    let mut ext = DuckDbQueryExt::default();
    ext.vector_filters = vec![VectorFilter {
        column: "v".into(),
        metric: "cosine".into(),
        vector: vec![0.0, 1.0, -1.0, 3.0],
        op: "Gte".into(),
        threshold: 0.5,
    }];
    let sql = translate(&q, Some(&ext)).unwrap();
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
    let sql = translate(&q, None).unwrap();
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
    let sql = translate(&q, None).unwrap();
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
    let sql = translate(&q, None).unwrap();
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
    let sql = translate(&q, None).unwrap();
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
    let result = translate(&q, None);
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
    let q = base_query("Fact");
    let mut ext = DuckDbQueryExt::default();
    ext.json_filters = vec![JsonFilter {
        column: "metadata".into(),
        path: "$.status".into(),
        op: "Ne".into(),
        value: Value::String("archived".into()),
    }];
    let sql = translate(&q, Some(&ext)).unwrap();
    assert_eq!(
        sql,
        "SELECT * FROM facts_view WHERE json_extract_string(metadata, '$.status') != 'archived'"
    );
}

#[test]
fn test_json_filter_lt() {
    let q = base_query("Fact");
    let mut ext = DuckDbQueryExt::default();
    ext.json_filters = vec![JsonFilter {
        column: "metadata".into(),
        path: "$.priority".into(),
        op: "Lt".into(),
        value: Value::Number(serde_json::Number::from(3)),
    }];
    let sql = translate(&q, Some(&ext)).unwrap();
    assert_eq!(
        sql,
        "SELECT * FROM facts_view WHERE json_extract_string(metadata, '$.priority') < 3"
    );
}

#[test]
fn test_json_filter_gte() {
    let q = base_query("Fact");
    let mut ext = DuckDbQueryExt::default();
    ext.json_filters = vec![JsonFilter {
        column: "metadata".into(),
        path: "$.confidence".into(),
        op: "Gte".into(),
        value: Value::Number(serde_json::Number::from(90)),
    }];
    let sql = translate(&q, Some(&ext)).unwrap();
    assert_eq!(
        sql,
        "SELECT * FROM facts_view WHERE json_extract_string(metadata, '$.confidence') >= 90"
    );
}

#[test]
fn test_json_filter_lte() {
    let q = base_query("Fact");
    let mut ext = DuckDbQueryExt::default();
    ext.json_filters = vec![JsonFilter {
        column: "metadata".into(),
        path: "$.revision".into(),
        op: "Lte".into(),
        value: Value::Number(serde_json::Number::from(5)),
    }];
    let sql = translate(&q, Some(&ext)).unwrap();
    assert_eq!(
        sql,
        "SELECT * FROM facts_view WHERE json_extract_string(metadata, '$.revision') <= 5"
    );
}

#[test]
fn test_json_filter_in_non_array_error() {
    let q = base_query("Fact");
    let mut ext = DuckDbQueryExt::default();
    ext.json_filters = vec![JsonFilter {
        column: "metadata".into(),
        path: "$.category".into(),
        op: "In".into(),
        value: Value::String("not_an_array".into()),
    }];
    let result = translate(&q, Some(&ext));
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
    let sql = translate(&q, None).unwrap();
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
    let sql = translate(&q, None).unwrap();
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
    let sql = translate(&q, None).unwrap();
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
    let sql = translate(&q, None).unwrap();
    assert_eq!(sql, "SELECT DISTINCT * FROM facts_view");
}

// ── Gap coverage: window function unknown func pass-through ────────────

#[test]
fn test_window_unknown_func_passthrough() {
    let mut q = base_query("Fact");
    q.projections = vec!["fact_id".into()];
    let mut ext = DuckDbQueryExt::default();
    ext.window_funcs = vec![WindowFuncDef {
        func: "NTILE".into(),
        column: Some("score".into()),
        partition_by: vec!["origin".into()],
        order_by: vec![],
        alias: Some("quartile".into()),
    }];
    let sql = translate(&q, Some(&ext)).unwrap();
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
        group_by: vec![],
        aggregates: vec![],
    });

    let q = base_query("Fact");
    let mut ext = DuckDbQueryExt::default();
    ext.with_ctes = vec![CteDef {
        alias: "bad_cte".into(),
        subquery: bad_sub,
    }];
    let result = translate(&q, Some(&ext));
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
    let sql = translate(&q, None).unwrap();
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
    let sql = translate(&q, None).unwrap();
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
    let sql = translate(&q, None).unwrap();
    assert_eq!(
        sql,
        "SELECT DISTINCT origin, COUNT(fact_id) AS cnt FROM facts_view"
    );
}
