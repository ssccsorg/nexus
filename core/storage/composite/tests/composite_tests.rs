// Integration tests for CompositeColdStorage with MockBlob + MockObject + MockKv (MetaStore).
//
// CompositeColdStorage no longer implements FactCapable, IntentCapable,
// HintCapable, StorageRead, or FilterCapable. Those are handled by
// PetgraphStorage (hot). This test suite covers the remaining traits:
//
//   - FlushCapable (cursor persistence)
//   - ScanCapable (blob archive scan)
//   - EvictCapable (blob eviction)
//   - TimeRangeCapable
//   - StorageRead (minimal, returns empty state)
//   - MetaStore (cursor position, snapshot pointers)

use nexus_model::{Content, EvictCapable, FlushCapable, FlushCursor, ScanCapable, StorageRead};
use nexus_storage_composite::{BlobStore, CompositeColdStorage, MetaStore};

mod common;
use common::{MockBlob, MockKv, MockObject};

fn storage() -> CompositeColdStorage<MockBlob, MockObject, MockKv> {
    CompositeColdStorage::new_with_system_clock(
        MockBlob::new(),
        MockObject::new(),
        MockKv::new(),
        "test-project",
    )
}

// ── StorageRead ──────────────────────────────────────────────────────────

#[test]
fn test_empty_storage() {
    let s = storage();
    let state = s.read_state();
    assert!(state.facts.is_empty(), "no facts expected");
    assert!(state.intents.is_empty(), "no intents expected");
    assert!(state.hints.is_empty(), "no hints expected");
}

#[test]
fn test_project_id() {
    let s = storage();
    assert_eq!(s.project_id(), "test-project");
}

// ── FlushCapable ─────────────────────────────────────────────────────────

#[test]
fn test_flush_empty_storage() {
    let s = storage();
    let cursor = FlushCursor::default();
    let result = s.flush_since(&cursor).expect("flush");
    assert_eq!(result.records_flushed, 0, "empty storage: 0 records");
    assert!(
        !result.new_cursor.last_flushed_at.is_empty(),
        "cursor updated"
    );
}

#[test]
fn test_cursor_persisted_to_meta() {
    let s = storage();
    let cursor = FlushCursor::default();
    let result = s.flush_since(&cursor).expect("flush");
    let saved = s
        .read_cursor()
        .expect("read cursor")
        .expect("cursor exists");
    assert_eq!(saved.last_flushed_at, result.new_cursor.last_flushed_at);
}

#[test]
fn test_incremental_flush() {
    let s = storage();
    let r1 = s.flush_since(&FlushCursor::default()).expect("first flush");
    assert_eq!(r1.records_flushed, 0, "first flush: 0");

    let r2 = s
        .flush_since(&FlushCursor {
            last_flushed_at: r1.new_cursor.last_flushed_at.clone(),
            partition: "default".into(),
        })
        .expect("second flush");
    assert_eq!(r2.records_flushed, 0, "incremental flush: 0 new records");
    assert!(
        r2.new_cursor.last_flushed_at > r1.new_cursor.last_flushed_at,
        "cursor advanced"
    );
}

#[test]
fn test_flush_with_partition() {
    let s = storage();
    let cursor = FlushCursor {
        last_flushed_at: String::new(),
        partition: "partition-x".into(),
    };
    let result = s.flush_since(&cursor).expect("flush with partition");
    assert_eq!(result.new_cursor.partition, "partition-x");
}

#[test]
fn test_flush_persists_data_to_blob() {
    // Manually write data to blob, then flush should count it.
    let s = storage();
    let fact = nexus_model::Fact {
        id: nexus_model::FihHash("f1".into()),
        origin: "t".into(),
        content: Content::Text("hello".into()),
        creator: "a".into(),
    };
    let bytes = postcard::to_allocvec(&fact).unwrap();
    s.blob()
        .put("test-project/flush/facts/default/100_0.bin", &bytes)
        .expect("put blob");

    let cursor = FlushCursor {
        partition: "default".into(),
        ..FlushCursor::default()
    };
    let result = s.flush_since(&cursor).expect("flush");
    assert!(result.records_flushed > 0, "blob data counted in flush");

    // Cursor persisted
    let saved = s
        .read_cursor()
        .expect("read cursor")
        .expect("cursor exists");
    assert_eq!(saved.last_flushed_at, result.new_cursor.last_flushed_at);
}

// ── ScanCapable ──────────────────────────────────────────────────────────

#[test]
fn test_scan_partition_empty() {
    let s = storage();
    let data = s.scan_partition("default").expect("scan");
    assert_eq!(data.facts.len(), 0);
    assert_eq!(data.intents.len(), 0);
    assert_eq!(data.hints.len(), 0);
    assert_eq!(data.partition, "default");
}

#[test]
fn test_scan_partition_with_data() {
    let s = storage();
    let fact = nexus_model::Fact {
        id: nexus_model::FihHash("f1".into()),
        origin: "t".into(),
        content: Content::Text("hello".into()),
        creator: "a".into(),
    };
    let bytes = postcard::to_allocvec(&fact).unwrap();
    s.blob()
        .put("test-project/flush/facts/default/100_0.bin", &bytes)
        .expect("put blob");

    let data = s.scan_partition("default").expect("scan");
    assert_eq!(data.facts.len(), 1, "one fact from blob");
    assert_eq!(data.facts[0].id.0, "f1");
}

// ── EvictCapable ─────────────────────────────────────────────────────────

#[test]
fn test_approximate_size_increases_with_data() {
    let s = storage();
    let empty_size = s.approximate_size();
    s.blob()
        .put("test-project/flush/facts/default/100_0.bin", b"data")
        .expect("put blob");
    let filled_size = s.approximate_size();
    assert!(filled_size > empty_size, "size grows with data");
}

#[test]
fn test_evict_before_removes_old_blobs() {
    let s = storage();
    s.blob()
        .put("test-project/flush/facts/default/100_0.bin", b"data")
        .expect("put blob");
    let count_before = s.blob().list("test-project/").unwrap().len();

    // Evict with a future timestamp
    let evicted = s.evict_before("9999999999999999999").expect("evict");
    assert_eq!(evicted, count_before as u64, "all blobs evicted");
    let count_after = s.blob().list("test-project/").unwrap().len();
    assert_eq!(count_after, 0, "no blobs remain");
}

#[test]
fn test_evict_before_keeps_recent_blobs() {
    let s = storage();
    s.blob()
        .put("test-project/flush/facts/default/100_0.bin", b"data")
        .expect("put blob");

    // Evict with timestamp 0 — should keep everything
    let evicted = s.evict_before("0").expect("evict");
    assert_eq!(evicted, 0, "no blobs evicted");
}

// ── MetaStore (via public accessors) ─────────────────────────────────────

#[test]
fn test_read_cursor_none_before_flush() {
    let s = storage();
    assert!(s.read_cursor().unwrap().is_none(), "no cursor before flush");
}

#[test]
fn test_flush_since_updates_cursor() {
    let s = storage();
    let r1 = s.flush_since(&FlushCursor::default()).expect("flush");
    let cursor = s.read_cursor().unwrap().expect("cursor after flush");
    assert_eq!(cursor.last_flushed_at, r1.new_cursor.last_flushed_at);
}

#[test]
fn test_empty_blob_list_does_not_crash() {
    let s = storage();
    let _ = s.blob().list("test-project/");
}
