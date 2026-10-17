// The walk over a volume: what it does with writes the medium has not taken, and with
// keys a channel lists but cannot hand over.
//
// The walk reads pages from the medium and then adds the session's own unflushed writes,
// so a caller sees the volume as the session knows it. A channel may also list a key it
// cannot read or whose bytes do not decode, and neither is a record to report.

#![cfg(feature = "std")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_executor::block_on;
use nex_fih::io::file_io::{FileIo, IoFuture};
use nex_fih::{AsyncFactCapable, Content, CoordId, Fact, FihStorage};

#[derive(Clone, Default)]
struct MemoryIo {
    map: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    refuse: Arc<Mutex<bool>>,
}

impl MemoryIo {
    fn refuse_writes(&self) {
        *self.refuse.lock().unwrap() = true;
    }
}

impl FileIo for MemoryIo {
    fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        let map = Arc::clone(&self.map);
        Box::pin(async move { Ok(map.lock().unwrap().get(path).cloned()) })
    }

    fn write<'a>(&'a self, path: &'a str, data: &'a [u8]) -> IoFuture<'a, ()> {
        let map = Arc::clone(&self.map);
        let refuse = Arc::clone(&self.refuse);
        Box::pin(async move {
            if *refuse.lock().unwrap() {
                return Err(format!("the medium refused the write: {path}"));
            }
            map.lock().unwrap().insert(path.to_string(), data.to_vec());
            Ok(())
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> IoFuture<'a, Vec<String>> {
        let map = Arc::clone(&self.map);
        Box::pin(async move {
            let mut keys: Vec<String> = map
                .lock()
                .unwrap()
                .keys()
                .filter(|key| key.starts_with(prefix))
                .cloned()
                .collect();
            keys.sort();
            Ok(keys)
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> IoFuture<'a, ()> {
        let map = Arc::clone(&self.map);
        Box::pin(async move {
            map.lock().unwrap().remove(path);
            Ok(())
        })
    }
}

/// A channel that lists one key and answers a read for it with what the test chose.
struct ListingIo {
    key: &'static str,
    bytes: Option<&'static [u8]>,
}

impl FileIo for ListingIo {
    fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        let answer = if path == self.key {
            self.bytes.map(|bytes| bytes.to_vec())
        } else {
            None
        };
        Box::pin(async move { Ok(answer) })
    }

    fn write<'a>(&'a self, _path: &'a str, _data: &'a [u8]) -> IoFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> IoFuture<'a, Vec<String>> {
        let keys = if prefix == "facts/" {
            vec![self.key.to_string()]
        } else {
            Vec::new()
        };
        Box::pin(async move { Ok(keys) })
    }

    fn delete<'a>(&'a self, _path: &'a str) -> IoFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}

fn fact(id: &str, payload: &str) -> Fact {
    Fact::with_id(
        CoordId::resolve(id),
        "origin/one".into(),
        Content::from(payload),
        "agent".into(),
    )
}

#[test]
fn a_framing_walk_reports_a_pending_fact_without_its_payload() {
    let medium = MemoryIo::default();
    let storage = FihStorage::new(medium.clone(), "walk");
    let fact = fact("f_pending", "the payload");
    block_on(storage.submit_fact(&fact)).expect("the fact is accepted");

    // The walk flushes first, so a medium that refuses leaves the write in the session.
    medium.refuse_writes();
    let mut seen: Vec<String> = Vec::new();
    block_on(storage.for_each_fact_record(|record| {
        seen.push(record.id.clone());
        true
    }))
    .expect("the walk runs");

    assert_eq!(seen, vec![fact.id.to_string()]);
}

#[test]
fn a_walk_that_stops_reports_the_first_pending_fact_and_returns() {
    let medium = MemoryIo::default();
    let storage = FihStorage::new(medium.clone(), "walk");
    for id in ["f_one", "f_two"] {
        block_on(storage.submit_fact(&fact(id, id))).expect("the fact is accepted");
    }
    medium.refuse_writes();

    let mut visited = 0;
    block_on(storage.for_each_fact(|_record, content| {
        assert!(!content.data.is_empty(), "the pending payload is available");
        visited += 1;
        false
    }))
    .expect("the walk runs");

    assert_eq!(visited, 1, "the visitor stopped the walk");
}

/// A medium that applies a prefix of the batch and then refuses the rest.
#[derive(Clone, Default)]
struct PartialIo {
    map: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    applied: Arc<Mutex<usize>>,
    fail_after: usize,
}

impl FileIo for PartialIo {
    fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        let map = Arc::clone(&self.map);
        Box::pin(async move { Ok(map.lock().unwrap().get(path).cloned()) })
    }

    fn write<'a>(&'a self, path: &'a str, data: &'a [u8]) -> IoFuture<'a, ()> {
        let map = Arc::clone(&self.map);
        let applied = Arc::clone(&self.applied);
        let fail_after = self.fail_after;
        Box::pin(async move {
            let mut applied = applied.lock().unwrap();
            if *applied >= fail_after {
                return Err(format!("the medium refused the write: {path}"));
            }
            *applied += 1;
            map.lock().unwrap().insert(path.to_string(), data.to_vec());
            Ok(())
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> IoFuture<'a, Vec<String>> {
        let map = Arc::clone(&self.map);
        Box::pin(async move {
            let mut keys: Vec<String> = map
                .lock()
                .unwrap()
                .keys()
                .filter(|key| key.starts_with(prefix))
                .cloned()
                .collect();
            keys.sort();
            Ok(keys)
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> IoFuture<'a, ()> {
        let map = Arc::clone(&self.map);
        Box::pin(async move {
            map.lock().unwrap().remove(path);
            Ok(())
        })
    }
}

#[test]
fn a_walk_reports_a_pending_record_the_partial_flush_left_on_the_medium_once() {
    // The first record's three ops reach the medium and the fourth is refused, so the
    // medium lists that record and the session still holds it. The walk must not report
    // it twice: the page path finds it in the pending buffer and the pending phase skips
    // it because the medium already holds it.
    let medium = PartialIo {
        fail_after: 3,
        ..Default::default()
    };
    let storage = FihStorage::new_without_maps(medium, "walk");
    let first = fact("f_one", "one");
    let second = fact("f_two", "two");
    block_on(storage.submit_fact(&first)).expect("the fact is accepted");
    block_on(storage.submit_fact(&second)).expect("the fact is accepted");

    let mut seen: Vec<String> = Vec::new();
    block_on(storage.for_each_fact_record(|record| {
        seen.push(record.id.clone());
        true
    }))
    .expect("the walk runs");

    seen.sort_unstable();
    let mut expected = vec![first.id.to_string(), second.id.to_string()];
    expected.sort_unstable();
    assert_eq!(seen, expected);
}

#[test]
fn a_listed_key_the_channel_cannot_read_is_skipped() {
    let storage = FihStorage::new_without_maps(
        ListingIo {
            key: "facts/f_ghost.fact",
            bytes: None,
        },
        "walk",
    );

    let mut visited = 0;
    block_on(storage.for_each_fact_record(|_record| {
        visited += 1;
        true
    }))
    .expect("the walk runs");

    assert_eq!(visited, 0);
}

#[test]
fn a_listed_key_whose_bytes_do_not_decode_is_skipped() {
    let storage = FihStorage::new_without_maps(
        ListingIo {
            key: "facts/f_garbled.fact",
            bytes: Some(b"not a record"),
        },
        "walk",
    );

    let mut visited = 0;
    block_on(storage.for_each_fact_record(|_record| {
        visited += 1;
        true
    }))
    .expect("the walk runs");

    assert_eq!(visited, 0);
}
