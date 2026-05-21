use nexus_model::{FilterCapable, ScanCapable, StateFilter, StorageRead, TimeRangeCapable};
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

// ── Test 8: time range ──────────────────────────────────────────────────

#[test]
fn test_time_range() {
    let tempdir = TempDir::new().unwrap();
    let facts_dir = tempdir.path().join("facts");
    std::fs::create_dir_all(&facts_dir).unwrap();
    let parquet_path = facts_dir.join("data.parquet").to_str().unwrap().to_string();

    // Build 10 facts spanning 2026-06-01 through 2026-06-10
    let mut sql = String::from(
        "SELECT 'fact_1' as fact_id, 'test' as origin, '\"a\"' as content, 'tester' as creator, '2026-06-01' as created_at",
    );
    for i in 2..=10 {
        let date = format!("2026-06-{:02}", i);
        sql.push_str(&format!(
            " UNION ALL SELECT 'fact_{}' as fact_id, 'test' as origin, '\"{}\"' as content, 'tester' as creator, '{}' as created_at",
            i, i, date
        ));
    }

    write_parquet(&sql, &parquet_path);

    let storage = DuckDbStorage::new(tempdir.path().to_str().unwrap()).unwrap();
    let range = storage.time_range();

    assert!(range.is_some(), "expected a time range");
    let r = range.unwrap();
    assert_eq!(r.start, "2026-06-01", "unexpected range start");
    assert_eq!(r.end, "2026-06-10", "unexpected range end");
}
