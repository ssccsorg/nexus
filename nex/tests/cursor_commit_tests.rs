// Cursor + meta store tests.
//
// Validates:
//   1. read_cursor returns the flush boundary from meta store
//   2. flush_since updates cursor in meta store
//   3. Parallel reads/writes do not race
//   4. MetaStore (KV) stores cursor and snapshot pointers

use nexus_model::{FlushCapable, FlushCursor, MetaStore};
use nexus_storage_composite::{
    AsyncStoreBlob, AsyncStoreObject, AsyncStoreSessionMeta, CompositeColdStorage,
};
use std::sync::Arc;
use std::thread;

fn storage() -> CompositeColdStorage<AsyncStoreBlob, AsyncStoreObject, AsyncStoreSessionMeta> {
    CompositeColdStorage::new_with_system_clock(
        AsyncStoreBlob::new(),
        AsyncStoreObject::new(),
        AsyncStoreSessionMeta::new(),
        "cqrs-test",
    )
}

#[test]
fn test_cursor_starts_empty() {
    let s = storage();
    let cursor = s.read_cursor().unwrap();
    assert!(cursor.is_none(), "cursor should be None before first flush");
}

#[test]
fn test_flush_updates_cursor() {
    let s = storage();

    // Flush with empty cursor (full flush)
    let cursor = FlushCursor {
        last_flushed_at: 0,
        partition: "p".into(),
    };
    let _ = s.flush_since(&cursor).unwrap();

    // Cursor should now be set in meta store
    let new_cursor = s
        .read_cursor()
        .unwrap()
        .expect("cursor should exist after flush");
    assert!(new_cursor.last_flushed_at > cursor.last_flushed_at);
}

#[test]
fn test_cursor_tracks_flush_boundary() {
    let s = storage();

    let r1 = s
        .flush_since(&FlushCursor {
            last_flushed_at: 0,
            partition: "p".into(),
        })
        .unwrap();
    let cursor1 = s.read_cursor().unwrap().unwrap();
    assert_eq!(cursor1.last_flushed_at, r1.new_cursor.last_flushed_at);

    // Second flush advances cursor
    let _r2 = s
        .flush_since(&FlushCursor {
            last_flushed_at: cursor1.last_flushed_at,
            partition: "p".into(),
        })
        .unwrap();
    let cursor2 = s.read_cursor().unwrap().unwrap();

    assert!(
        cursor2.last_flushed_at > cursor1.last_flushed_at,
        "cursor should advance after each flush"
    );
    assert!(
        cursor2.last_flushed_at != cursor1.last_flushed_at,
        "cursors should be different"
    );
}

#[test]
fn test_concurrent_read_during_flush() {
    let s = Arc::new(storage());

    // Writer thread
    let sw = Arc::clone(&s);
    let writer = thread::spawn(move || {
        for _ in 0..5 {
            let _ = sw.flush_since(&FlushCursor {
                last_flushed_at: 0,
                partition: "p".into(),
            });
        }
    });

    // Reader thread
    let sr = Arc::clone(&s);
    let reader = thread::spawn(move || {
        for _ in 0..50 {
            let _ = sr.read_cursor();
        }
    });

    writer.join().unwrap();
    reader.join().unwrap();
}

#[test]
fn test_incremental_flush_respects_cursor() {
    let s = storage();

    let r1 = s
        .flush_since(&FlushCursor {
            last_flushed_at: 0,
            partition: "p".into(),
        })
        .unwrap();
    assert_eq!(
        r1.records_flushed, 0,
        "first flush on empty storage: 0 records"
    );
    let c1 = r1.new_cursor.last_flushed_at;

    // Second flush with same cursor should also be 0 (no new data)
    let r2 = s
        .flush_since(&FlushCursor {
            last_flushed_at: c1,
            partition: "p".into(),
        })
        .unwrap();
    assert_eq!(r2.records_flushed, 0, "second flush: 0 records");

    // cursor was updated (flush_since always writes new cursor)
    let saved = s.read_cursor().unwrap().unwrap();
    assert!(saved.last_flushed_at > 0, "cursor should exist after flush");
}

#[test]
fn test_meta_store_get_set() {
    // Verify that the meta store (AsyncStoreSessionMeta) works correctly
    // for storing cursor and snapshot pointer values.
    let meta = AsyncStoreSessionMeta::new();

    meta.set("cursor", "20260530_123456").unwrap();
    assert_eq!(meta.get("cursor").unwrap(), Some("20260530_123456".into()));

    meta.set("snapshot_ts", "20260530_120000").unwrap();
    assert_eq!(
        meta.get("snapshot_ts").unwrap(),
        Some("20260530_120000".into())
    );

    // Overwrite
    meta.set("cursor", "20260530_123457").unwrap();
    assert_eq!(meta.get("cursor").unwrap(), Some("20260530_123457".into()));
}
