// nexus-storage-duckdb — Incremental flush tests.
//
// Verifies that FlushCursor correctly tracks last_flushed_at so that
// repeated flushes export only newly-ingested data rather than duplicating
// all data into new Parquet files.

use nexus_model::{FlushCapable, FlushCursor, FlushResult, StorageRead};
use nexus_storage_duckdb::DuckDbStorage;
use tempfile::TempDir;

/// Helper: create a DuckDbStorage with an empty temp directory.
fn empty_storage() -> (DuckDbStorage, TempDir) {
    let tempdir = TempDir::new().unwrap();
    let base = tempdir.path().to_str().unwrap().to_string();
    // Create empty entity directories so DuckDB views compile.
    std::fs::create_dir_all(tempdir.path().join("facts")).unwrap();
    std::fs::create_dir_all(tempdir.path().join("intents")).unwrap();
    std::fs::create_dir_all(tempdir.path().join("hints")).unwrap();
    let storage = DuckDbStorage::new(&base, "test").unwrap();
    (storage, tempdir)
}

/// Helper: submit a fact directly via DuckDB COPY ... TO so it appears
/// in the facts_view. (DuckDbStorage does not implement FactCapable.)
fn inject_fact_parquet(storage: &DuckDbStorage, fact_id: &str, created_at: &str) {
    let conn = storage.conn.lock().unwrap();
    let facts_dir = format!("{}/facts/partition=test", storage.base_path);
    let _ = std::fs::create_dir_all(&facts_dir);
    let path = format!("{}/{}.parquet", facts_dir, fact_id);
    let sql = format!(
        "COPY (SELECT '{}' as fact_id, 'test' as origin, '\"data\"' as content, 'tester' as creator, '{}' as created_at) TO '{}' (FORMAT PARQUET);",
        fact_id, created_at, path
    );
    let _ = conn.execute(&sql);
    // Refresh view to include the new file.
    let glob = format!("{}/facts/**/*.parquet", storage.base_path);
    let _ = conn.execute(&format!(
        "CREATE OR REPLACE VIEW facts_view AS SELECT * FROM read_parquet('{}', union_by_name=true);",
        glob
    ));
}

// ── Test 1: first flush with empty cursor exports all data ──────────────

#[test]
fn test_flush_full_export_with_empty_cursor() {
    let (storage, _tempdir) = empty_storage();

    // Inject initial data
    inject_fact_parquet(&storage, "f001", "2026-06-01");
    inject_fact_parquet(&storage, "f002", "2026-06-02");

    // Full flush: cursor.last_flushed_at is empty
    let cursor = FlushCursor {
        last_flushed_at: String::new(),
        partition: "test".to_string(),
    };
    let FlushResult {
        records_flushed,
        new_cursor,
    } = storage.flush_since(&cursor).unwrap();

    // Both facts should be exported
    assert_eq!(records_flushed, 2, "full flush should export 2 facts");
    assert!(
        !new_cursor.last_flushed_at.is_empty(),
        "cursor should have a timestamp after full flush"
    );
    assert_eq!(new_cursor.partition, "test");
}

// ── Test 2: incremental flush exports only newer data ──────────────────

#[test]
fn test_incremental_flush_exports_only_new_data() {
    let (storage, _tempdir) = empty_storage();

    // Phase 1: inject old data and flush
    inject_fact_parquet(&storage, "f_old_1", "2026-06-01");
    inject_fact_parquet(&storage, "f_old_2", "2026-06-02");

    let cursor = FlushCursor {
        last_flushed_at: String::new(),
        partition: "test".to_string(),
    };
    let FlushResult { new_cursor, .. } = storage.flush_since(&cursor).unwrap();
    let first_ts = new_cursor.last_flushed_at.clone();

    // Phase 2: inject newer data and flush with cursor from phase 1
    inject_fact_parquet(&storage, "f_new_1", "2026-06-10");

    let FlushResult {
        records_flushed,
        new_cursor: cursor2,
    } = storage.flush_since(&FlushCursor {
        last_flushed_at: first_ts,
        partition: "test".to_string(),
    })
    .unwrap();

    // Only the new fact should be exported; old facts already flushed
    assert_eq!(
        records_flushed, 1,
        "incremental flush should export only 1 new fact"
    );
    assert!(
        cursor2.last_flushed_at > first_ts,
        "cursor timestamp should advance"
    );
}

// ── Test 3: flush with same cursor twice produces no duplicate exports ──

#[test]
fn test_repeated_flush_no_duplicate() {
    let (storage, _tempdir) = empty_storage();

    // Inject one fact
    inject_fact_parquet(&storage, "f_only", "2026-06-01");

    // First flush — full export
    let cursor = FlushCursor {
        last_flushed_at: String::new(),
        partition: "test".to_string(),
    };
    let FlushResult {
        records_flushed: first_count,
        new_cursor,
    } = storage.flush_since(&cursor).unwrap();
    assert_eq!(first_count, 1, "first flush exports 1 fact");

    // Second flush with same cursor — no new data, 0 exported
    let FlushResult {
        records_flushed: second_count,
        ..
    } = storage.flush_since(&FlushCursor {
        last_flushed_at: new_cursor.last_flushed_at.clone(),
        partition: "test".to_string(),
    })
    .unwrap();
    assert_eq!(
        second_count, 0,
        "second flush should export 0 facts (no new data)"
    );

    // Third flush with a cursor that points to a time before the fact
    let FlushResult {
        records_flushed: third_count,
        ..
    } = storage.flush_since(&FlushCursor {
        last_flushed_at: "2026-05-01".to_string(),
        partition: "test".to_string(),
    })
    .unwrap();
    assert_eq!(
        third_count, 1,
        "flush with older cursor re-exports the fact"
    );
}

// ── Test 4: flush with non-empty cursor but no new data returns 0 ──────

#[test]
fn test_flush_with_future_cursor_returns_zero() {
    let (storage, _tempdir) = empty_storage();

    inject_fact_parquet(&storage, "f_old", "2026-06-01");

    // Cursor pointing to a timestamp after all data
    let cursor = FlushCursor {
        last_flushed_at: "2099-12-31".to_string(),
        partition: "test".to_string(),
    };
    let FlushResult {
        records_flushed,
        new_cursor,
    } = storage.flush_since(&cursor).unwrap();

    assert_eq!(
        records_flushed, 0,
        "no data newer than 2099, should export 0"
    );
    assert!(
        !new_cursor.last_flushed_at.is_empty(),
        "cursor still updated with current timestamp"
    );
}

// ── Test 5: flush produces readable Parquet files ──────────────────────

#[test]
fn test_flushed_parquet_is_readable() {
    let (storage, _tempdir) = empty_storage();

    inject_fact_parquet(&storage, "f_a", "2026-06-01");
    inject_fact_parquet(&storage, "f_b", "2026-06-02");

    // Flush — this writes Parquet files to {base_path}/facts/partition=test/
    let cursor = FlushCursor {
        last_flushed_at: String::new(),
        partition: "test".to_string(),
    };
    let FlushResult { new_cursor, .. } = storage.flush_since(&cursor).unwrap();

    // Read state back — should see all facts (both from initial inject
    // and flushed files since they share the same base_path glob)
    let state = storage.read_state();
    assert_eq!(
        state.facts.len(),
        2,
        "should read 2 facts after flush (inject + flush files)"
    );
    assert!(!new_cursor.last_flushed_at.is_empty());

    // Create a fresh storage instance from the same base path to verify
    // that flushed Parquet files persist independently of the in-memory state.
    let storage2 = DuckDbStorage::new(&storage.base_path, "test").unwrap();
    let state2 = storage2.read_state();
    assert_eq!(
        state2.facts.len(),
        2,
        "fresh instance reads 2 facts from flushed Parquet"
    );
}

// ── Test 6: full flush then incremental — total line count matches ──────

#[test]
fn test_incremental_flush_data_completeness() {
    let (storage, _tempdir) = empty_storage();

    // Batch 1: 3 facts
    inject_fact_parquet(&storage, "f_b1_a", "2026-06-01");
    inject_fact_parquet(&storage, "f_b1_b", "2026-06-02");
    inject_fact_parquet(&storage, "f_b1_c", "2026-06-03");

    let cursor = FlushCursor {
        last_flushed_at: String::new(),
        partition: "test".to_string(),
    };
    let FlushResult {
        records_flushed: batch1,
        new_cursor: c1,
    } = storage.flush_since(&cursor).unwrap();
    assert_eq!(batch1, 3, "batch 1 exports 3 facts");

    // Batch 2: 2 more facts
    inject_fact_parquet(&storage, "f_b2_a", "2026-06-10");
    inject_fact_parquet(&storage, "f_b2_b", "2026-06-11");

    let FlushResult {
        records_flushed: batch2,
        new_cursor: c2,
    } = storage
        .flush_since(&c1)
        .unwrap();
    assert_eq!(batch2, 2, "batch 2 exports 2 facts");

    // Batch 3: 1 more fact
    inject_fact_parquet(&storage, "f_b3_a", "2026-06-20");

    let FlushResult {
        records_flushed: batch3,
        ..
    } = storage
        .flush_since(&c2)
        .unwrap();
    assert_eq!(batch3, 1, "batch 3 exports 1 fact");

    // Total exported across all flushes = 3 + 2 + 1 = 6
    // (Each inject also writes a file, so total files in facts/ will be
    //  more than 6; but flushed copies are 6 distinct Parquet rows.)
    // Verify via fresh instance.
    let storage2 = DuckDbStorage::new(&storage.base_path, "test").unwrap();
    let state = storage2.read_state();
    assert_eq!(
        state.facts.len(),
        6,
        "all 6 facts should be readable from Parquet"
    );
}

// ── Test 7: flush with different partitions is isolated ─────────────────

#[test]
fn test_flush_partition_isolation() {
    let tempdir = TempDir::new().unwrap();
    let base = tempdir.path().to_str().unwrap().to_string();
    std::fs::create_dir_all(tempdir.path().join("facts")).unwrap();
    std::fs::create_dir_all(tempdir.path().join("intents")).unwrap();
    std::fs::create_dir_all(tempdir.path().join("hints")).unwrap();

    let storage_a = DuckDbStorage::new(&base, "project_a").unwrap();
    let storage_b = DuckDbStorage::new(&base, "project_b").unwrap();

    // Inject data for project_a
    inject_fact_parquet(&storage_a, "f_a1", "2026-06-01");
    let cursor_a = FlushCursor {
        last_flushed_at: String::new(),
        partition: "project_a".to_string(),
    };
    let FlushResult {
        records_flushed: a_count,
        ..
    } = storage_a.flush_since(&cursor_a).unwrap();
    assert_eq!(a_count, 1, "project_a flush exports 1 fact");

    // Inject data for project_b
    inject_fact_parquet(&storage_b, "f_b1", "2026-06-01");
    let cursor_b = FlushCursor {
        last_flushed_at: String::new(),
        partition: "project_b".to_string(),
    };
    let FlushResult {
        records_flushed: b_count,
        ..
    } = storage_b.flush_since(&cursor_b).unwrap();
    assert_eq!(b_count, 1, "project_b flush exports 1 fact");

    // Fresh instance reads all (6 = 3 inject + 3 flush files)
    let storage_all = DuckDbStorage::new(&base, "all").unwrap();
    let state = storage_all.read_state();
    assert_eq!(
        state.facts.len(),
        2,
        "both projects' facts readable from same base path"
    );
}
