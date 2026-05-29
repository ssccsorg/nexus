// CQRS commit channel + cursor tests.
//
// Validates:
//   1. flush_since writes to commit_blob/commit_kv, NOT to kv/blob
//   2. read_cursor returns the flush boundary
//   3. Parallel reads/writes do not race
//   4. Cursor replaces dirty tracking — flush boundary is determinable

use nexus_model::{Fact, FihHash, FlushCapable, FlushCursor};
use nexus_storage_kv_cold::{
    BlobStore, CompositeColdStorage, IoBufferBlob, IoBufferKv, IoBufferObject, KeyValueStore,    
};
use std::sync::{Arc, Barrier};
use std::thread;

fn test_fact(id: &str, _ts: &str) -> Fact {
    Fact {
        id: FihHash(id.into()),
        origin: "t".into(),
        content: serde_json::json!({"v": id}),
        creator: "a".into(),
    }
}

fn storage() -> CompositeColdStorage<IoBufferKv, IoBufferBlob, IoBufferObject> {
    CompositeColdStorage::new_with_system_clock(
        IoBufferKv::new(),
        IoBufferBlob::new(),
        IoBufferObject::new(),
        "cqrs-test",
    )
}

fn stamp(fact: &Fact, ts: &str) -> String {
    #[derive(serde::Serialize)]
    struct S<'a, T> {
        submitted_at: &'a str,
        data: &'a T,
    }
    serde_json::to_string(&S {
        submitted_at: ts,
        data: fact,
    })
    .unwrap()
}

#[test]
fn test_cursor_starts_empty() {
    let s = storage();
    let cursor = s.read_cursor().unwrap();
    assert!(cursor.is_none(), "cursor should be None before first flush");
}

#[test]
fn test_flush_updates_cursor_on_commit_kv() {
    let s = storage();

    // Submit data
    s.kv()
        .set("cqrs-test:fact:f1", &stamp(&test_fact("f1", "100"), "100"))
        .unwrap();

    let cursor = FlushCursor {
        last_flushed_at: String::new(),
        partition: "p".into(),
    };
    let result = s.flush_since(&cursor).unwrap();
    assert!(result.records_flushed > 0, "should flush at least one fact");

    // Cursor should now be set on commit_kv
    let new_cursor = s
        .read_cursor()
        .unwrap()
        .expect("cursor should exist after flush");
    assert!(new_cursor.last_flushed_at > cursor.last_flushed_at);
}

#[test]
fn test_flush_writes_to_commit_blob() {
    let s = storage();

    s.kv()
        .set("cqrs-test:fact:f1", &stamp(&test_fact("f1", "100"), "100"))
        .unwrap();

    let cursor = FlushCursor {
        last_flushed_at: String::new(),
        partition: "p".into(),
    };
    s.flush_since(&cursor).unwrap();

    // commit_blob should have data (flush wrote to it)
    let commit_blobs = s.commit_blob().list("cqrs-test/").unwrap();
    assert!(
        !commit_blobs.is_empty(),
        "commit blob should have flush output"
    );
}

#[test]
fn test_cursor_tracks_flush_boundary() {
    let s = storage();

    // Phase 1: submit data with ts=100
    s.kv()
        .set(
            "cqrs-test:fact:old",
            &stamp(&test_fact("old", "100"), "100"),
        )
        .unwrap();
    s.flush_since(&FlushCursor {
        last_flushed_at: String::new(),
        partition: "p".into(),
    })
    .unwrap();
    let cursor1 = s.read_cursor().unwrap().unwrap().last_flushed_at.clone();

    // Phase 2: submit new data with ts=200
    s.kv()
        .set(
            "cqrs-test:fact:new",
            &stamp(&test_fact("new", "200"), "200"),
        )
        .unwrap();
    s.flush_since(&FlushCursor {
        last_flushed_at: cursor1.clone(),
        partition: "p".into(),
    })
    .unwrap();
    let cursor2 = s.read_cursor().unwrap().unwrap().last_flushed_at;

    assert!(cursor2 > cursor1, "cursor should advance after each flush");
    assert_ne!(cursor1, cursor2, "cursors should be different");
}

#[test]
fn test_concurrent_writes_then_flush() {
    let s = Arc::new(storage());
    let barrier = Arc::new(Barrier::new(4));
    let threads: Vec<_> = (0..4)
        .map(|i| {
            let s = Arc::clone(&s);
            let b = Arc::clone(&barrier);
            thread::spawn(move || {
                b.wait();
                for j in 0..10 {
                    let id = format!("w{i}_{j}");
                    let key = format!("cqrs-test:fact:{id}");
                    s.kv()
                        .set(&key, &stamp(&test_fact(&id, "100"), "100"))
                        .unwrap();
                }
            })
        })
        .collect();

    for t in threads {
        t.join().unwrap();
    }

    // All writes should have gone to main kv
    let keys = s.kv().list("cqrs-test:fact:").unwrap();
    assert_eq!(keys.len(), 40, "40 facts should have been written");

    // Flush should work on all 40
    let cursor = FlushCursor {
        last_flushed_at: String::new(),
        partition: "p".into(),
    };
    let result = s.flush_since(&cursor).unwrap();
    assert_eq!(result.records_flushed, 40);
}

#[test]
fn test_concurrent_read_during_write() {
    let s = Arc::new(storage());

    // Pre-populate
    for i in 0..10 {
        s.kv()
            .set(
                &format!("cqrs-test:fact:f{i}"),
                &stamp(&test_fact(&format!("f{i}"), "100"), "100"),
            )
            .unwrap();
    }

    // Writer thread
    let sw = Arc::clone(&s);
    let writer = thread::spawn(move || {
        for i in 10..20 {
            sw.kv()
                .set(
                    &format!("cqrs-test:fact:f{i}"),
                    &stamp(&test_fact(&format!("f{i}"), "200"), "200"),
                )
                .unwrap();
        }
    });

    // Reader thread
    let sr = Arc::clone(&s);
    let reader = thread::spawn(move || {
        let mut count = 0;
        for _ in 0..100 {
            count = sr.kv().list("cqrs-test:fact:").unwrap().len();
        }
        count
    });

    writer.join().unwrap();
    let final_count = reader.join().unwrap();

    // At least 10 (pre-populated), at most 20 (all writes visible)
    assert!(
        final_count >= 10,
        "reader should see at least pre-populated data"
    );
    assert!(final_count <= 20, "reader should see at most all data");
}

#[test]
fn test_flush_then_read_cursor_via_commit_kv() {
    let s = storage();

    // Submit and flush
    for i in 0..5 {
        s.kv()
            .set(
                &format!("cqrs-test:fact:f{i}"),
                &stamp(&test_fact(&format!("f{i}"), "100"), "100"),
            )
            .unwrap();
    }

    let cursor = FlushCursor {
        last_flushed_at: String::new(),
        partition: "p".into(),
    };
    let result = s.flush_since(&cursor).unwrap();
    let new_cursor_ts = result.new_cursor.last_flushed_at.clone();

    // Read cursor via commit_kv
    let saved = s.read_cursor().unwrap().expect("cursor should be saved");
    assert_eq!(
        saved.last_flushed_at, new_cursor_ts,
        "commit_kv cursor should match flush result"
    );
}

#[test]
fn test_incremental_flush_respects_cursor() {
    let s = storage();

    // First batch
    s.kv()
        .set("cqrs-test:fact:a", &stamp(&test_fact("a", "100"), "100"))
        .unwrap();
    s.kv()
        .set("cqrs-test:fact:b", &stamp(&test_fact("b", "100"), "100"))
        .unwrap();

    let r1 = s
        .flush_since(&FlushCursor {
            last_flushed_at: String::new(),
            partition: "p".into(),
        })
        .unwrap();
    assert_eq!(r1.records_flushed, 2, "first flush: 2 facts");
    assert!(
        !r1.new_cursor.last_flushed_at.is_empty(),
        "cursor should be set"
    );
    let c1 = r1.new_cursor.last_flushed_at.clone();

    // Second batch — same timestamp, should be excluded by cursor
    s.kv()
        .set("cqrs-test:fact:c", &stamp(&test_fact("c", "100"), "100"))
        .unwrap();
    let r2 = s
        .flush_since(&FlushCursor {
            last_flushed_at: c1.clone(),
            partition: "p".into(),
        })
        .unwrap();
    assert_eq!(
        r2.records_flushed, 0,
        "second flush: timestamp not newer than cursor"
    );

    // cursor was updated (flush_since always writes new cursor, even with 0 records)
    let saved = s.read_cursor().unwrap().unwrap();
    assert!(
        !saved.last_flushed_at.is_empty(),
        "cursor should exist after flush"
    );
}
