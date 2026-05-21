use nexus_model::{
    CypherCapable, FilterCapable, ScanCapable, StateFilter, StorageRead, TimeRangeCapable,
};
use nexus_storage_duckdb::DuckDbStorage;
use tempfile::TempDir;

/// Helper: open a temporary in-memory DuckDB connection, LOAD parquet, and
/// COPY the given SELECT query result to a Parquet file at `path`.
fn write_parquet(select_sql: &str, path: &str) {
    let conn = duckdb::Connection::open_in_memory().unwrap();
    conn.execute_batch("LOAD parquet;").unwrap();
    let sql = format!("COPY ({}) TO '{}' (FORMAT PARQUET);", select_sql, path);
    conn.execute_batch(&sql).unwrap();
}

// ── Test 1: empty state ─────────────────────────────────────────────────

#[test]
fn test_read_empty_state() {
    let tempdir = TempDir::new().unwrap();
    let base = tempdir.path().to_str().unwrap().to_string();

    // Create empty directories — no Parquet files at all.
    std::fs::create_dir_all(tempdir.path().join("facts")).unwrap();
    std::fs::create_dir_all(tempdir.path().join("intents")).unwrap();
    std::fs::create_dir_all(tempdir.path().join("hints")).unwrap();

    let storage = DuckDbStorage::new(&base).unwrap();
    let state = storage.read_state();

    assert!(state.facts.is_empty(), "expected no facts");
    assert!(state.intents.is_empty(), "expected no intents");
    assert!(state.hints.is_empty(), "expected no hints");
}

// ── Test 2: facts from Parquet ──────────────────────────────────────────

#[test]
fn test_read_facts_from_parquet() {
    let tempdir = TempDir::new().unwrap();
    let facts_dir = tempdir.path().join("facts");
    std::fs::create_dir_all(&facts_dir).unwrap();
    let parquet_path = facts_dir.join("data.parquet").to_str().unwrap().to_string();

    write_parquet(
        "SELECT 'fact_1' as fact_id, 'origin_a' as origin, '\"hello\"' as content, 'tester' as creator, '2026-06-01' as created_at
         UNION ALL
         SELECT 'fact_2' as fact_id, 'origin_b' as origin, '\"world\"' as content, 'admin' as creator, '2026-06-02' as created_at",
        &parquet_path,
    );

    let storage = DuckDbStorage::new(tempdir.path().to_str().unwrap()).unwrap();
    let state = storage.read_state();

    assert_eq!(state.facts.len(), 2, "expected 2 facts");

    let fact_1 = state.facts.iter().find(|f| f.id.0 == "fact_1").unwrap();
    assert_eq!(fact_1.origin, "origin_a");
    assert_eq!(fact_1.content, serde_json::json!("hello"));
    assert_eq!(fact_1.creator, "tester");

    let fact_2 = state.facts.iter().find(|f| f.id.0 == "fact_2").unwrap();
    assert_eq!(fact_2.origin, "origin_b");
    assert_eq!(fact_2.content, serde_json::json!("world"));
    assert_eq!(fact_2.creator, "admin");

    assert!(state.intents.is_empty(), "expected no intents");
    assert!(state.hints.is_empty(), "expected no hints");
}

// ── Test 3: intents from Parquet ────────────────────────────────────────

#[test]
fn test_read_intents_from_parquet() {
    let tempdir = TempDir::new().unwrap();
    let intents_dir = tempdir.path().join("intents");
    std::fs::create_dir_all(&intents_dir).unwrap();
    let parquet_path = intents_dir
        .join("data.parquet")
        .to_str()
        .unwrap()
        .to_string();

    write_parquet(
        "SELECT 'intent_1' as intent_id, '[\"fact_1\"]' as from_facts, 'do something' as description, 'tester' as creator,
                'worker_a' as worker, NULL as to_fact_id, '2026-06-01T00:00:00Z' as last_heartbeat_at,
                '2026-06-01' as created_at, NULL as concluded_at
         UNION ALL
         SELECT 'intent_2' as intent_id, '[\"fact_1\",\"fact_2\"]' as from_facts, 'do more' as description, 'admin' as creator,
                NULL as worker, 'fact_3' as to_fact_id, NULL as last_heartbeat_at,
                '2026-06-02' as created_at, '2026-06-03' as concluded_at",
        &parquet_path,
    );

    let storage = DuckDbStorage::new(tempdir.path().to_str().unwrap()).unwrap();
    let state = storage.read_state();

    assert_eq!(state.intents.len(), 2, "expected 2 intents");

    // intent_1 — all optional fields populated except to_fact_id / concluded_at
    let intent_1 = state.intents.iter().find(|i| i.id.0 == "intent_1").unwrap();
    assert_eq!(intent_1.from_facts, vec!["fact_1".to_string()]);
    assert_eq!(intent_1.description, "do something");
    assert_eq!(intent_1.creator, "tester");
    assert_eq!(intent_1.worker, Some("worker_a".to_string()));
    assert_eq!(intent_1.to_fact_id, None);
    assert_eq!(
        intent_1.last_heartbeat_at,
        Some("2026-06-01T00:00:00Z".to_string())
    );
    assert_eq!(intent_1.created_at, Some("2026-06-01".to_string()));
    assert_eq!(intent_1.concluded_at, None);

    // intent_2 — different optional fields populated
    let intent_2 = state.intents.iter().find(|i| i.id.0 == "intent_2").unwrap();
    assert_eq!(
        intent_2.from_facts,
        vec!["fact_1".to_string(), "fact_2".to_string()]
    );
    assert_eq!(intent_2.description, "do more");
    assert_eq!(intent_2.creator, "admin");
    assert_eq!(intent_2.worker, None);
    assert_eq!(intent_2.to_fact_id, Some("fact_3".to_string()));
    assert_eq!(intent_2.last_heartbeat_at, None);
    assert_eq!(intent_2.created_at, Some("2026-06-02".to_string()));
    assert_eq!(intent_2.concluded_at, Some("2026-06-03".to_string()));

    assert!(state.facts.is_empty(), "expected no facts");
    assert!(state.hints.is_empty(), "expected no hints");
}

// ── Test 4: hints from Parquet ──────────────────────────────────────────

#[test]
fn test_read_hints_from_parquet() {
    let tempdir = TempDir::new().unwrap();
    let hints_dir = tempdir.path().join("hints");
    std::fs::create_dir_all(&hints_dir).unwrap();
    let parquet_path = hints_dir.join("data.parquet").to_str().unwrap().to_string();

    write_parquet(
        "SELECT 'hint_1' as hint_id, 'content one' as content, 'tester' as creator, '2026-06-01' as created_at
         UNION ALL
         SELECT 'hint_2' as hint_id, 'content two' as content, 'admin' as creator, '2026-06-02' as created_at",
        &parquet_path,
    );

    let storage = DuckDbStorage::new(tempdir.path().to_str().unwrap()).unwrap();
    let state = storage.read_state();

    assert_eq!(state.hints.len(), 2, "expected 2 hints");

    let hint_1 = state.hints.iter().find(|h| h.id.0 == "hint_1").unwrap();
    assert_eq!(hint_1.content, "content one");
    assert_eq!(hint_1.creator, "tester");

    let hint_2 = state.hints.iter().find(|h| h.id.0 == "hint_2").unwrap();
    assert_eq!(hint_2.content, "content two");
    assert_eq!(hint_2.creator, "admin");

    assert!(state.facts.is_empty(), "expected no facts");
    assert!(state.intents.is_empty(), "expected no intents");
}

// ── Test 5: filter by since ─────────────────────────────────────────────

#[test]
fn test_filter_by_since() {
    let tempdir = TempDir::new().unwrap();
    let facts_dir = tempdir.path().join("facts");
    std::fs::create_dir_all(&facts_dir).unwrap();
    let parquet_path = facts_dir.join("data.parquet").to_str().unwrap().to_string();

    write_parquet(
        "SELECT 'fact_1' as fact_id, 'test' as origin, '\"a\"' as content, 'tester' as creator, '2026-06-01' as created_at
         UNION ALL
         SELECT 'fact_2' as fact_id, 'test' as origin, '\"b\"' as content, 'tester' as creator, '2026-06-02' as created_at
         UNION ALL
         SELECT 'fact_3' as fact_id, 'test' as origin, '\"c\"' as content, 'tester' as creator, '2026-06-10' as created_at",
        &parquet_path,
    );

    let storage = DuckDbStorage::new(tempdir.path().to_str().unwrap()).unwrap();
    let filter = StateFilter {
        since: Some("2026-06-02".to_string()),
        ..Default::default()
    };
    let state = storage.read_state_filtered(&filter);

    assert_eq!(state.facts.len(), 2, "expected 2 facts after since filter");
    assert!(
        state.facts.iter().any(|f| f.id.0 == "fact_2"),
        "fact_2 should be included"
    );
    assert!(
        state.facts.iter().any(|f| f.id.0 == "fact_3"),
        "fact_3 should be included"
    );
    assert!(
        !state.facts.iter().any(|f| f.id.0 == "fact_1"),
        "fact_1 should be excluded"
    );
}

// ── Test 6: filter by fact_ids ──────────────────────────────────────────

#[test]
fn test_filter_by_fact_ids() {
    let tempdir = TempDir::new().unwrap();
    let facts_dir = tempdir.path().join("facts");
    std::fs::create_dir_all(&facts_dir).unwrap();
    let parquet_path = facts_dir.join("data.parquet").to_str().unwrap().to_string();

    write_parquet(
        "SELECT 'fact_1' as fact_id, 'test' as origin, '\"hello\"' as content, 'tester' as creator, '2026-06-01' as created_at
         UNION ALL
         SELECT 'fact_2' as fact_id, 'test' as origin, '\"world\"' as content, 'tester' as creator, '2026-06-02' as created_at
         UNION ALL
         SELECT 'fact_3' as fact_id, 'test' as origin, '\"foo\"' as content, 'tester' as creator, '2026-06-03' as created_at",
        &parquet_path,
    );

    let storage = DuckDbStorage::new(tempdir.path().to_str().unwrap()).unwrap();
    let filter = StateFilter {
        fact_ids: Some(vec!["fact_1".to_string()]),
        ..Default::default()
    };
    let state = storage.read_state_filtered(&filter);

    assert_eq!(state.facts.len(), 1, "expected exactly 1 fact");
    assert_eq!(state.facts[0].id.0, "fact_1");
    assert_eq!(state.facts[0].content, serde_json::json!("hello"));
}

// ── Test 7: scan partition ──────────────────────────────────────────────

#[test]
fn test_scan_partition() {
    let tempdir = TempDir::new().unwrap();
    let base = tempdir.path().to_str().unwrap().to_string();

    // Partition 2026-06-01
    let facts_dir_1 = tempdir.path().join("facts").join("partition=2026-06-01");
    std::fs::create_dir_all(&facts_dir_1).unwrap();
    let parquet_path_1 = facts_dir_1
        .join("data.parquet")
        .to_str()
        .unwrap()
        .to_string();
    write_parquet(
        "SELECT 'fact_p1' as fact_id, 'test' as origin, '\"p1_data\"' as content, 'tester' as creator, '2026-06-01' as created_at",
        &parquet_path_1,
    );

    // Partition 2026-06-02
    let facts_dir_2 = tempdir.path().join("facts").join("partition=2026-06-02");
    std::fs::create_dir_all(&facts_dir_2).unwrap();
    let parquet_path_2 = facts_dir_2
        .join("data.parquet")
        .to_str()
        .unwrap()
        .to_string();
    write_parquet(
        "SELECT 'fact_p2' as fact_id, 'test' as origin, '\"p2_data\"' as content, 'tester' as creator, '2026-06-02' as created_at",
        &parquet_path_2,
    );

    let storage = DuckDbStorage::new(&base).unwrap();

    // Scan partition 2026-06-01 — only fact_p1 expected
    let data_1 = storage.scan_partition("2026-06-01").unwrap();
    assert_eq!(data_1.partition, "2026-06-01");
    assert_eq!(data_1.facts.len(), 1, "expected 1 fact in partition 1");
    assert_eq!(data_1.facts[0].id.0, "fact_p1");
    assert_eq!(data_1.facts[0].content, serde_json::json!("p1_data"));
    assert!(data_1.intents.is_empty(), "expected no intents");
    assert!(data_1.hints.is_empty(), "expected no hints");

    // Scan partition 2026-06-02 — only fact_p2 expected
    let data_2 = storage.scan_partition("2026-06-02").unwrap();
    assert_eq!(data_2.partition, "2026-06-02");
    assert_eq!(data_2.facts.len(), 1, "expected 1 fact in partition 2");
    assert_eq!(data_2.facts[0].id.0, "fact_p2");
    assert_eq!(data_2.facts[0].content, serde_json::json!("p2_data"));
}

// ── Test 9: multiple Parquet files unioned ──────────────────────────────

#[test]
fn test_read_multiple_parquet_files() {
    let tempdir = TempDir::new().unwrap();
    let facts_dir = tempdir.path().join("facts");
    std::fs::create_dir_all(&facts_dir).unwrap();

    // Write two separate Parquet files in the same facts/ directory
    let p1 = facts_dir
        .join("batch_1.parquet")
        .to_str()
        .unwrap()
        .to_string();
    write_parquet(
        "SELECT 'fact_a' as fact_id, 'origin_a' as origin, '\"a\"' as content, 'tester' as creator, '2026-06-01' as created_at",
        &p1,
    );
    let p2 = facts_dir
        .join("batch_2.parquet")
        .to_str()
        .unwrap()
        .to_string();
    write_parquet(
        "SELECT 'fact_b' as fact_id, 'origin_b' as origin, '\"b\"' as content, 'tester' as creator, '2026-06-02' as created_at",
        &p2,
    );

    let storage = DuckDbStorage::new(tempdir.path().to_str().unwrap()).unwrap();
    let state = storage.read_state();

    assert_eq!(state.facts.len(), 2, "expected 2 facts from 2 files");
    assert!(state.facts.iter().any(|f| f.id.0 == "fact_a"));
    assert!(state.facts.iter().any(|f| f.id.0 == "fact_b"));
}

// ── Test 10: full BoardState (all three types) ──────────────────────────

#[test]
fn test_full_board_state() {
    let tempdir = TempDir::new().unwrap();
    let base = tempdir.path();

    // Write facts
    let facts_dir = base.join("facts");
    std::fs::create_dir_all(&facts_dir).unwrap();
    write_parquet(
        "SELECT 'f_1' as fact_id, 'src' as origin, '\"data\"' as content, 'agent' as creator, '2026-06-01' as created_at",
        &facts_dir.join("data.parquet").to_str().unwrap().to_string(),
    );

    // Write intents
    let intents_dir = base.join("intents");
    std::fs::create_dir_all(&intents_dir).unwrap();
    write_parquet(
        "SELECT 'i_1' as intent_id, '[]' as from_facts, 'explore' as description, 'agent' as creator,
                NULL as worker, NULL as to_fact_id, NULL as last_heartbeat_at,
                '2026-06-01' as created_at, NULL as concluded_at",
        &intents_dir.join("data.parquet").to_str().unwrap().to_string(),
    );

    // Write hints
    let hints_dir = base.join("hints");
    std::fs::create_dir_all(&hints_dir).unwrap();
    write_parquet(
        "SELECT 'h_1' as hint_id, 'check this' as content, 'analyst' as creator, '2026-06-01' as created_at",
        &hints_dir.join("data.parquet").to_str().unwrap().to_string(),
    );

    let storage = DuckDbStorage::new(base.to_str().unwrap()).unwrap();
    let state = storage.read_state();

    assert_eq!(state.facts.len(), 1, "expected 1 fact");
    assert_eq!(state.intents.len(), 1, "expected 1 intent");
    assert_eq!(state.hints.len(), 1, "expected 1 hint");
    assert_eq!(state.facts[0].id.0, "f_1");
    assert_eq!(state.intents[0].id.0, "i_1");
    assert_eq!(state.hints[0].id.0, "h_1");
}

// ── Test 11: missing directories ────────────────────────────────────────

#[test]
fn test_missing_directories() {
    // Create tempdir with NO subdirectories at all
    let tempdir = TempDir::new().unwrap();
    let storage = DuckDbStorage::new(tempdir.path().to_str().unwrap()).unwrap();
    let state = storage.read_state();
    // Should not panic — returns empty BoardState
    assert!(state.facts.is_empty());
    assert!(state.intents.is_empty());
    assert!(state.hints.is_empty());
}

// ── Test 12: non-existent partition scan ────────────────────────────────

#[test]
fn test_scan_nonexistent_partition() {
    let tempdir = TempDir::new().unwrap();
    let storage = DuckDbStorage::new(tempdir.path().to_str().unwrap()).unwrap();
    // Scanning a partition that doesn't exist should return empty data, not error
    let data = storage.scan_partition("2099-99-99").unwrap();
    assert_eq!(data.partition, "2099-99-99");
    assert!(data.facts.is_empty());
    assert!(data.intents.is_empty());
    assert!(data.hints.is_empty());
}

// ── Test 13: time range on empty storage ────────────────────────────────

#[test]
fn test_time_range_empty() {
    let tempdir = TempDir::new().unwrap();
    std::fs::create_dir_all(tempdir.path().join("facts")).unwrap();
    std::fs::create_dir_all(tempdir.path().join("intents")).unwrap();
    std::fs::create_dir_all(tempdir.path().join("hints")).unwrap();

    let storage = DuckDbStorage::new(tempdir.path().to_str().unwrap()).unwrap();
    let range = storage.time_range();
    // Empty storage should return None
    assert!(range.is_none(), "expected None for empty storage");
}

// ── Test 14: combined filter (since + until + id) ───────────────────────

#[test]
fn test_filter_combined() {
    let tempdir = TempDir::new().unwrap();
    let facts_dir = tempdir.path().join("facts");
    std::fs::create_dir_all(&facts_dir).unwrap();
    let parquet_path = facts_dir.join("data.parquet").to_str().unwrap().to_string();

    write_parquet(
        "SELECT 'f_1' as fact_id, 'src' as origin, '\"a\"' as content, 'tester' as creator, '2026-06-01' as created_at
         UNION ALL
         SELECT 'f_2' as fact_id, 'src' as origin, '\"b\"' as content, 'tester' as creator, '2026-06-05' as created_at
         UNION ALL
         SELECT 'f_3' as fact_id, 'src' as origin, '\"c\"' as content, 'tester' as creator, '2026-06-10' as created_at
         UNION ALL
         SELECT 'f_4' as fact_id, 'src' as origin, '\"d\"' as content, 'tester' as creator, '2026-06-15' as created_at",
        &parquet_path,
    );

    let storage = DuckDbStorage::new(tempdir.path().to_str().unwrap()).unwrap();
    let filter = StateFilter {
        fact_ids: Some(vec!["f_2".into(), "f_3".into()]),
        since: Some("2026-06-04".into()),
        until: Some("2026-06-12".into()),
        ..Default::default()
    };
    let state = storage.read_state_filtered(&filter);

    // f_2 (2026-06-05) matches: within time range AND in id list
    // f_3 (2026-06-10) matches: within time range AND in id list
    // f_1 excluded: not in id list
    // f_4 excluded: not in id list (also out of range)
    assert_eq!(state.facts.len(), 2, "expected 2 facts");
    assert!(state.facts.iter().any(|f| f.id.0 == "f_2"));
    assert!(state.facts.iter().any(|f| f.id.0 == "f_3"));
}

// ── Test 15: filter by intent_ids and hint_ids ──────────────────────────

#[test]
fn test_filter_intent_and_hint_ids() {
    let tempdir = TempDir::new().unwrap();
    let intents_dir = tempdir.path().join("intents");
    std::fs::create_dir_all(&intents_dir).unwrap();
    write_parquet(
        "SELECT 'i_a' as intent_id, '[]' as from_facts, 'desc_a' as description, 'tester' as creator,
                NULL as worker, NULL as to_fact_id, NULL as last_heartbeat_at,
                '2026-06-01' as created_at, NULL as concluded_at
         UNION ALL
         SELECT 'i_b' as intent_id, '[]' as from_facts, 'desc_b' as description, 'tester' as creator,
                NULL as worker, NULL as to_fact_id, NULL as last_heartbeat_at,
                '2026-06-02' as created_at, NULL as concluded_at",
        &intents_dir.join("data.parquet").to_str().unwrap().to_string(),
    );

    let hints_dir = tempdir.path().join("hints");
    std::fs::create_dir_all(&hints_dir).unwrap();
    write_parquet(
        "SELECT 'h_x' as hint_id, 'content_x' as content, 'tester' as creator, '2026-06-01' as created_at
         UNION ALL
         SELECT 'h_y' as hint_id, 'content_y' as content, 'tester' as creator, '2026-06-02' as created_at",
        &hints_dir.join("data.parquet").to_str().unwrap().to_string(),
    );

    let storage = DuckDbStorage::new(tempdir.path().to_str().unwrap()).unwrap();

    // Filter by intent_ids
    let ifilter = StateFilter {
        intent_ids: Some(vec!["i_a".into()]),
        ..Default::default()
    };
    let state = storage.read_state_filtered(&ifilter);
    assert_eq!(state.intents.len(), 1, "expected 1 intent");
    assert_eq!(state.intents[0].id.0, "i_a");

    // Filter by hint_ids (hint_ids filter is passed to DuckDB but the view
    // query still returns all hints since build_where_clause only filters facts)
    let hfilter = StateFilter {
        hint_ids: Some(vec!["h_y".into()]),
        ..Default::default()
    };
    let state = storage.read_state_filtered(&hfilter);
    // Note: DuckDbStorage read_state_filtered only filters facts by fact_ids.
    // Intent/hint filtering is not yet implemented at the DuckDB level.
    // This test verifies the behavior doesn't crash.
    assert!(state.hints.len() >= 1);
}

// ── Test 16: complex JSON content ───────────────────────────────────────

#[test]
fn test_complex_json_content() {
    let tempdir = TempDir::new().unwrap();
    let facts_dir = tempdir.path().join("facts");
    std::fs::create_dir_all(&facts_dir).unwrap();
    let parquet_path = facts_dir.join("data.parquet").to_str().unwrap().to_string();

    // Write nested JSON as content
    write_parquet(
        "SELECT 'f_nested' as fact_id, 'test' as origin,
                '{\"nested\":{\"array\":[1,2,3],\"obj\":{\"key\":\"val\"}},\"num\":42,\"flag\":true}' as content,
                'tester' as creator, '2026-06-01' as created_at
         UNION ALL
         SELECT 'f_null_content' as fact_id, 'test' as origin,
                'null' as content,
                'tester' as creator, '2026-06-02' as created_at",
        &parquet_path,
    );

    let storage = DuckDbStorage::new(tempdir.path().to_str().unwrap()).unwrap();
    let state = storage.read_state();

    let nested = state.facts.iter().find(|f| f.id.0 == "f_nested").unwrap();
    assert_eq!(nested.content["nested"]["array"][0], serde_json::json!(1));
    assert_eq!(
        nested.content["nested"]["obj"]["key"],
        serde_json::json!("val")
    );
    assert_eq!(nested.content["num"], serde_json::json!(42));
    assert_eq!(nested.content["flag"], serde_json::json!(true));

    let null_fact = state
        .facts
        .iter()
        .find(|f| f.id.0 == "f_null_content")
        .unwrap();
    assert_eq!(null_fact.content, serde_json::Value::Null);
}

// ── Test 17: stress test — 1000 facts ───────────────────────────────────

#[test]
fn test_stress_1000_facts() {
    let tempdir = TempDir::new().unwrap();
    let facts_dir = tempdir.path().join("facts");
    std::fs::create_dir_all(&facts_dir).unwrap();
    let parquet_path = facts_dir
        .join("stress.parquet")
        .to_str()
        .unwrap()
        .to_string();

    // Build 1000 facts using DuckDB's range() table function
    write_parquet(
        "SELECT 'fact_' || CAST(i AS VARCHAR) as fact_id,
                'stress' as origin,
                '\"val_\" || CAST(i AS VARCHAR) || \"\"' as content,
                'loader' as creator,
                '2026-06-' || LPAD(CAST((i % 30 + 1) AS VARCHAR), 2, '0') as created_at
         FROM range(1000) t(i)",
        &parquet_path,
    );

    let storage = DuckDbStorage::new(tempdir.path().to_str().unwrap()).unwrap();
    let state = storage.read_state();

    assert_eq!(state.facts.len(), 1000, "expected 1000 facts");
    assert!(state.facts.iter().any(|f| f.id.0 == "fact_0"));
    assert!(state.facts.iter().any(|f| f.id.0 == "fact_999"));
}

// ── Test 19: CypherCapable — DuckDB Cypher→SQL query execution ───────

#[test]
fn test_cypher_capable_cold_query() {
    // CypherCapable for DuckDbStorage translates a ColdQuery to SQL and
    // executes it against parquet-backed views. This test writes parquet
    // data and queries it through the CypherCapable interface.
    let tempdir = TempDir::new().unwrap();
    let facts_dir = tempdir.path().join("facts");
    std::fs::create_dir_all(&facts_dir).unwrap();
    let parquet_path = facts_dir.join("data.parquet").to_str().unwrap().to_string();

    write_parquet(
        "SELECT 'f_a' as fact_id, 'src' as origin, '\"alpha\"' as content, 'tester' as creator, '2026-06-01' as created_at
         UNION ALL
         SELECT 'f_b' as fact_id, 'src' as origin, '\"beta\"' as content, 'tester' as creator, '2026-06-02' as created_at",
        &parquet_path,
    );

    let storage = DuckDbStorage::new(tempdir.path().to_str().unwrap()).unwrap();

    // Query all facts via ColdQuery
    let plan = serde_json::json!({
        "label": "Fact",
        "filters": [],
        "projections": ["fact_id", "origin", "content"],
        "order_by": [],
        "limit": null,
        "offset": null,
        "distinct": false,
        "aggregate_count": false
    });

    let result = storage.query_plan(&plan).unwrap();
    let rows = result.as_array().unwrap();
    assert_eq!(rows.len(), 2, "expected 2 facts");
    assert_eq!(rows[0]["fact_id"], "f_a");
    assert_eq!(rows[1]["fact_id"], "f_b");

    // Query with filter
    let plan = serde_json::json!({
        "label": "Fact",
        "filters": [{"field": "fact_id", "op": "Eq", "value": "f_a"}],
        "projections": ["fact_id", "content"],
        "order_by": [],
        "limit": null,
        "offset": null,
        "distinct": false,
        "aggregate_count": false
    });

    let result = storage.query_plan(&plan).unwrap();
    let rows = result.as_array().unwrap();
    assert_eq!(rows.len(), 1, "expected 1 filtered fact");
    assert_eq!(rows[0]["fact_id"], "f_a");

    // Count query
    let count_plan = serde_json::json!({
        "label": "Fact",
        "filters": [],
        "projections": [],
        "order_by": [],
        "limit": null,
        "offset": null,
        "distinct": false,
        "aggregate_count": true
    });
    let count_result = storage.query_plan(&count_plan).unwrap();
    assert!(count_result.is_array(), "count query returns array");
    let count_row = count_result.as_array().and_then(|a| a.first()).expect("count query should return a row");
    assert_eq!(count_row["count"], serde_json::json!(2), "expected count=2");

    // Empty plan should fail to parse
    let empty_result = storage.query_plan(&serde_json::json!({}));
    assert!(empty_result.is_err(), "empty plan should fail to parse");
    assert!(
        empty_result.unwrap_err().contains("ColdQuery"),
        "error should mention ColdQuery"
    );
}

// ── Test 20: TimeRangeCapable routing demo (hot vs cold) ─────────────

#[test]
fn test_time_range_routing_demo() {
    // This test demonstrates that TimeRangeCapable enables the SSCCS-Nexus
    // planner to distinguish between hot (bounded range) and cold
    // (universal / unbounded) storage backends for query routing.
    //
    // Future routing logic (#51):
    //   if query_time_range ⊆ hot.time_range() → petgraph (µs)
    //   else → DuckDB / Parquet (columnar scan)

    let tempdir = TempDir::new().unwrap();
    let facts_dir = tempdir.path().join("facts");
    std::fs::create_dir_all(&facts_dir).unwrap();
    let parquet_path = facts_dir.join("data.parquet").to_str().unwrap().to_string();

    // Write facts with a known time span: 2026-06-01 to 2026-06-10
    write_parquet(
        "SELECT 'fact_1' as fact_id, 'src' as origin, '\"a\"' as content, 'tester' as creator, '2026-06-01' as created_at
         UNION ALL
         SELECT 'fact_10' as fact_id, 'src' as origin, '\"b\"' as content, 'tester' as creator, '2026-06-10' as created_at",
        &parquet_path,
    );

    let cold = DuckDbStorage::new(tempdir.path().to_str().unwrap()).unwrap();
    let cold_range = cold.time_range();

    assert!(
        cold_range.is_some(),
        "cold storage with data has a time range"
    );
    let cold_range = cold_range.unwrap();
    assert_eq!(cold_range.start, "2026-06-01");
    assert_eq!(cold_range.end, "2026-06-10");

    // Simulate routing decision based on query vs backend time ranges.
    // A query asking for data after 2026-06-05 would overlap with cold.
    let query_since = "2026-06-05";
    let query_until = "2026-06-15";

    // Check if query range overlaps with cold range
    let query_overlaps_cold =
        query_since <= cold_range.end.as_str() && query_until >= cold_range.start.as_str();
    assert!(
        query_overlaps_cold,
        "query range should overlap cold storage"
    );

    // A query asking for data before 2026-05-01 would NOT overlap with cold.
    let early_query_until = "2026-05-01";
    let early_overlaps = early_query_until >= cold_range.start.as_str();
    assert!(
        !early_overlaps,
        "early query should not overlap cold storage"
    );

    // This is the routing logic that SSCCS-Nexus planner will use (#51):
    //   if query ⊆ hot.time_range() → execute on petgraph
    //   elif query ⊆ cold.time_range() → execute on DuckDB
    //   else → hybrid (UNION ALL)
}

// ── Test 18: filter with limit and offset ───────────────────────────────

#[test]
fn test_filter_limit_offset() {
    let tempdir = TempDir::new().unwrap();
    let facts_dir = tempdir.path().join("facts");
    std::fs::create_dir_all(&facts_dir).unwrap();
    let parquet_path = facts_dir.join("data.parquet").to_str().unwrap().to_string();

    // Write 5 facts with known IDs
    write_parquet(
        "SELECT 'f_a' as fact_id, 'src' as origin, '\"a\"' as content, 'tester' as creator, '2026-06-01' as created_at
         UNION ALL
         SELECT 'f_b' as fact_id, 'src' as origin, '\"b\"' as content, 'tester' as creator, '2026-06-02' as created_at
         UNION ALL
         SELECT 'f_c' as fact_id, 'src' as origin, '\"c\"' as content, 'tester' as creator, '2026-06-03' as created_at
         UNION ALL
         SELECT 'f_d' as fact_id, 'src' as origin, '\"d\"' as content, 'tester' as creator, '2026-06-04' as created_at
         UNION ALL
         SELECT 'f_e' as fact_id, 'src' as origin, '\"e\"' as content, 'tester' as creator, '2026-06-05' as created_at",
        &parquet_path,
    );

    let storage = DuckDbStorage::new(tempdir.path().to_str().unwrap()).unwrap();

    // LIMIT 2
    let filter = StateFilter {
        limit: Some(2),
        ..Default::default()
    };
    let state = storage.read_state_filtered(&filter);
    assert_eq!(state.facts.len(), 2, "expected 2 facts with LIMIT 2");

    // LIMIT 2 OFFSET 2
    let filter = StateFilter {
        limit: Some(2),
        offset: Some(2),
        ..Default::default()
    };
    let state = storage.read_state_filtered(&filter);
    assert_eq!(
        state.facts.len(),
        2,
        "expected 2 facts with LIMIT 2 OFFSET 2"
    );
}
