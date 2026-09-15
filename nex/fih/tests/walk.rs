// Walking a volume one record at a time.
//
// The record maps answer a read by materializing the whole state, which a device with
// tens of kilobytes cannot afford: 394 to 406 bytes per record on riscv32. The walk is
// the read path that costs one record instead. What it has to get right is the order (the
// identifier order a state read reports), the content (a walk that hands over records but
// not their payloads answers nothing), the writes a session has not flushed, and the stop
// a question about one record depends on.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_executor::block_on;
use nex_fih::io::file_io::{FileIo, IoFuture};
use nex_fih::{AsyncFactCapable, AsyncStorageRead, Content, Fact, FihStorage};

/// An in-memory medium, shared by every storage that holds a clone of it.
///
/// A clone is the same medium rather than a copy, which is what lets a test write a
/// volume in one session and read it in the next.
#[derive(Clone, Default)]
struct MemoryIo {
    map: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl FileIo for MemoryIo {
    fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        let map = Arc::clone(&self.map);
        Box::pin(async move { Ok(map.lock().unwrap().get(path).cloned()) })
    }

    fn write<'a>(&'a self, path: &'a str, data: &'a [u8]) -> IoFuture<'a, ()> {
        let map = Arc::clone(&self.map);
        Box::pin(async move {
            map.lock().unwrap().insert(path.to_string(), data.to_vec());
            Ok(())
        })
    }

    /// Lists the way a region lists: every key under the prefix, in key order.
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

/// The identifiers a walk visited, and the bytes each record's content held.
fn walk(storage: &FihStorage<MemoryIo>) -> (Vec<String>, Vec<usize>) {
    let mut ids: Vec<String> = Vec::new();
    let mut sizes: Vec<usize> = Vec::new();
    block_on(storage.for_each_fact(|record, content| {
        ids.push(record.id.clone());
        sizes.push(content.data.len());
        true
    }))
    .expect("the walk reaches the medium");
    (ids, sizes)
}

fn submit(storage: &FihStorage<MemoryIo>, payload: &str, origin: &str) {
    let fact = Fact::new(origin.into(), Content::from(payload), "walker".into());
    block_on(storage.submit_fact(&fact)).expect("the fact is accepted");
}

#[test]
fn a_walk_sees_a_volume_another_session_wrote() {
    let medium = MemoryIo::default();
    let writer = FihStorage::new(medium.clone(), "walk");
    submit(&writer, "first payload", "origin/one");
    submit(&writer, "second payload", "origin/one");
    submit(&writer, "third payload", "origin/two");
    block_on(writer.flush_pending()).expect("the writes reach the medium");

    // A second session over the same medium, with nothing read into memory yet: this is
    // the device that is asked about a past it did not write.
    let reader = FihStorage::new(medium, "walk");
    let (ids, mut sizes) = walk(&reader);

    assert_eq!(ids.len(), 3, "a walk sees every record: {ids:?}");

    // The order is the identifier order and not the order the records were written, so
    // what a test can hold is which payloads came back and not the sequence they did.
    let mut expected = vec![
        "first payload".len(),
        "second payload".len(),
        "third payload".len(),
    ];
    sizes.sort_unstable();
    expected.sort_unstable();
    assert_eq!(
        sizes, expected,
        "a walk hands over the content of each record"
    );
}

#[test]
fn a_walk_visits_in_the_order_a_state_read_reports() {
    let medium = MemoryIo::default();
    let writer = FihStorage::new(medium.clone(), "walk");
    for payload in ["one", "two", "three", "four", "five"] {
        submit(&writer, payload, "origin/order");
    }
    block_on(writer.flush_pending()).expect("the writes reach the medium");

    let reader = FihStorage::new(medium, "walk");
    block_on(reader.rebuild_cache()).expect("the cache is rebuilt");
    let reported: Vec<String> = block_on(reader.read_state())
        .facts
        .iter()
        .map(|fact| fact.id.to_string())
        .collect();
    let (walked, _) = walk(&reader);

    // The two orders are one order. A device compares a walk against a state read of the
    // same volume, and a difference here would make the two disagree about the volume
    // rather than about the order of it.
    assert_eq!(walked, reported, "a walk and a state read report one order");
}

#[test]
fn a_walk_sees_writes_this_session_has_not_flushed() {
    let medium = MemoryIo::default();
    let storage = FihStorage::new(medium, "walk");
    submit(&storage, "unflushed", "origin/pending");
    submit(&storage, "also unflushed", "origin/pending");

    // No flush: the walk is what has to reach the medium for them, and it does.
    let (ids, _) = walk(&storage);

    assert_eq!(
        ids.len(),
        2,
        "a walk sees the session's own writes: {ids:?}"
    );
}

#[test]
fn a_walk_stops_when_the_visitor_says_so() {
    let medium = MemoryIo::default();
    let writer = FihStorage::new(medium.clone(), "walk");
    for payload in ["one", "two", "three"] {
        submit(&writer, payload, "origin/stop");
    }
    block_on(writer.flush_pending()).expect("the writes reach the medium");

    let reader = FihStorage::new(medium, "walk");
    let mut visited = 0;
    block_on(reader.for_each_fact(|_record, _content| {
        visited += 1;
        false
    }))
    .expect("the walk reaches the medium");

    // One, not three: a question about one record reads one record, which is what makes
    // the work of a walk the records the question reaches.
    assert_eq!(visited, 1, "the walk stopped at the first record");
}
