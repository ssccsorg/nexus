// Walking a volume one record at a time.
//
// The record maps answer a read by materializing the whole state, which a device with tens of
// kilobytes cannot afford: 394 to 406 bytes per record on riscv32. The walk is the read path that
// costs a page and a record instead. What it has to get right is the set of records (a walk and a
// state read of one volume reach the same records), the content (a walk that hands over records
// but not their payloads answers nothing), the writes a session has not flushed, the stop a
// question about one record depends on, and the page it reads the keys in, because a page is what
// keeps the walk's memory from being the volume's.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_executor::block_on;
use nex_fih::io::file_io::{FileIo, IoFuture};
use nex_fih::{AsyncFactCapable, AsyncStorageRead, Content, Fact, FactRecord, FihStorage};

/// An in-memory medium, shared by every storage that holds a clone of it.
///
/// A clone is the same medium rather than a copy, which is what lets a test write a
/// volume in one session and read it in the next. It also keeps the keys every read asked
/// for, which is how a test says that a walk did not read something.
#[derive(Clone, Default)]
struct MemoryIo {
    map: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    reads: Arc<Mutex<Vec<String>>>,
    /// Set to refuse every write, which is a part at the end of its life.
    refuse: Arc<Mutex<bool>>,
}

impl FileIo for MemoryIo {
    fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        let map = Arc::clone(&self.map);
        let reads = Arc::clone(&self.reads);
        Box::pin(async move {
            reads.lock().unwrap().push(path.to_string());
            Ok(map.lock().unwrap().get(path).cloned())
        })
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
fn walk<IO: FileIo>(storage: &FihStorage<IO>) -> (Vec<String>, Vec<usize>) {
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

fn submit<IO: FileIo>(storage: &FihStorage<IO>, payload: &str, origin: &str) {
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

    // The walk promises no order, and this medium hands its keys over in key order, so what a
    // test can hold is which payloads came back.
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

/// A walk and a state read of one volume reach the same records.
///
/// The walk promises no order, so what the two have to agree about is the volume rather than the
/// order of it. A device reads a volume with the walk and a host with the maps, and a difference
/// here would make the two disagree about the volume.
#[test]
fn a_walk_and_a_state_read_reach_the_same_records() {
    let medium = MemoryIo::default();
    let writer = FihStorage::new(medium.clone(), "walk");
    for payload in ["one", "two", "three", "four", "five"] {
        submit(&writer, payload, "origin/same");
    }
    block_on(writer.flush_pending()).expect("the writes reach the medium");

    let reader = FihStorage::new(medium, "walk");
    block_on(reader.rebuild_cache()).expect("the cache is rebuilt");
    let mut reported: Vec<String> = block_on(reader.read_state())
        .facts
        .iter()
        .map(|fact| fact.id.to_string())
        .collect();
    let (mut walked, _) = walk(&reader);

    reported.sort_unstable();
    walked.sort_unstable();
    assert_eq!(walked, reported, "a walk and a state read reach one set");
}

/// The entries of a [`PagedIo`], in the order they were written, shared by every clone.
type Entries = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

/// A medium that hands over a prefix's keys a page at a time, in the reverse of the order they were
/// written, which is neither the key order nor the write order.
///
/// Two things follow from that. The walk promises no order, so it visits in the order the channel
/// serves, which a medium that served key order could not show. And `list` hands over nothing, so a
/// walk that read the list rather than the pages would visit no record at all.
#[derive(Clone, Default)]
struct PagedIo {
    entries: Entries,
    /// The bound each page was asked for, which is the walk's page rather than the volume's.
    asked: Arc<Mutex<Vec<usize>>>,
}

impl FileIo for PagedIo {
    fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        let entries = Arc::clone(&self.entries);
        Box::pin(async move {
            Ok(entries
                .lock()
                .unwrap()
                .iter()
                .find(|(key, _)| key == path)
                .map(|(_, data)| data.clone()))
        })
    }

    fn write<'a>(&'a self, path: &'a str, data: &'a [u8]) -> IoFuture<'a, ()> {
        let entries = Arc::clone(&self.entries);
        Box::pin(async move {
            let mut entries = entries.lock().unwrap();
            match entries.iter_mut().find(|(key, _)| key == path) {
                Some(slot) => slot.1 = data.to_vec(),
                None => entries.push((path.to_string(), data.to_vec())),
            }
            Ok(())
        })
    }

    fn list<'a>(&'a self, _prefix: &'a str) -> IoFuture<'a, Vec<String>> {
        // A channel that pages does not owe the list.
        Box::pin(async { Ok(Vec::new()) })
    }

    fn list_page<'a>(
        &'a self,
        prefix: &'a str,
        cursor: Option<&'a [u8]>,
        max: usize,
    ) -> IoFuture<'a, (Vec<String>, Option<Vec<u8>>)> {
        let entries = Arc::clone(&self.entries);
        let asked = Arc::clone(&self.asked);
        Box::pin(async move {
            asked.lock().unwrap().push(max);
            let start = match cursor {
                Some(bytes) => {
                    u32::from_le_bytes(bytes.try_into().expect("a four-byte cursor")) as usize
                }
                None => 0,
            };
            let matching: Vec<String> = entries
                .lock()
                .unwrap()
                .iter()
                .rev()
                .map(|(key, _)| key.clone())
                .filter(|key| key.starts_with(prefix))
                .collect();
            let page: Vec<String> = matching.iter().skip(start).take(max).cloned().collect();
            let next = start + page.len();
            let token = (next < matching.len()).then(|| (next as u32).to_le_bytes().to_vec());
            Ok((page, token))
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> IoFuture<'a, ()> {
        let entries = Arc::clone(&self.entries);
        Box::pin(async move {
            entries.lock().unwrap().retain(|(key, _)| key != path);
            Ok(())
        })
    }
}

/// A walk holds a page of keys rather than the volume, and it visits in the channel's order because
/// that is the order it was given.
#[test]
fn a_walk_reads_the_keys_a_page_at_a_time_in_the_channel_s_order() {
    let medium = PagedIo::default();
    let writer = FihStorage::new(medium.clone(), "walk");
    for i in 0..40 {
        submit(&writer, &format!("payload {i}"), "origin/pages");
    }
    block_on(writer.flush_pending()).expect("the writes reach the medium");

    let reader = FihStorage::new(medium.clone(), "walk");
    let (visited, _) = walk(&reader);
    let visited: Vec<String> = visited.iter().map(|id| FactRecord::fact_key(id)).collect();

    // The bounds the walk asked for, read before the fixture below asks for one of its own.
    let asked = medium.asked.lock().unwrap().clone();

    // What the channel serves, in the order it serves it.
    let served = block_on(medium.list_page("facts/", None, usize::MAX))
        .expect("the page")
        .0;
    let mut sorted = served.clone();
    sorted.sort();
    assert_ne!(
        served, sorted,
        "the fixture must serve an order that is not key order"
    );
    assert_eq!(visited, served, "the walk visits in the channel's order");

    // Forty records over a page the walk asked for, so more than one page and a bound that is
    // the page rather than the volume.
    assert!(asked.len() > 1, "the walk read one page: {asked:?}");
    assert!(
        asked.iter().all(|max| *max <= 32),
        "the walk asked for a bound that is not a page: {asked:?}"
    );
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

#[test]
fn a_framing_walk_visits_the_same_records_without_reading_their_content() {
    let medium = MemoryIo::default();
    let writer = FihStorage::new(medium.clone(), "walk");
    for payload in ["one", "two", "three"] {
        submit(&writer, payload, "origin/framing");
    }
    block_on(writer.flush_pending()).expect("the writes reach the medium");

    let reader = FihStorage::new(medium.clone(), "walk");
    let (with_content, _) = walk(&reader);

    let before = medium.reads.lock().unwrap().len();
    let mut with_framing: Vec<String> = Vec::new();
    block_on(reader.for_each_fact_record(|record| {
        with_framing.push(record.id.clone());
        true
    }))
    .expect("the walk reaches the medium");

    // One visit order, and the same records either way.
    assert_eq!(
        with_framing, with_content,
        "the two walks visit the same records in the same order"
    );

    // And the difference the framing walk exists for: it never asks the medium for a
    // payload, so the reads it made are the record keys and nothing else. The content
    // walk above read a payload and its metadata for each record, which is the work this
    // one does not do.
    let asked: Vec<String> = medium.reads.lock().unwrap()[before..].to_vec();
    let payloads = asked.iter().filter(|key| key.contains("blob/")).count();
    assert_eq!(payloads, 0, "a framing walk read a payload: {asked:?}");
    assert_eq!(
        asked.len(),
        with_content.len(),
        "a framing walk reads one key per record: {asked:?}"
    );
}

/// A part at the end of its life refuses the write, and the walk still answers.
///
/// A refused flush must not make the volume unreadable: the writes the medium refused stay
/// pending and are visited from memory. Without that, a worn part turns a record the
/// session is still holding into one that is gone.
#[test]
fn a_walk_visits_writes_the_medium_refused() {
    let medium = MemoryIo::default();
    *medium.refuse.lock().unwrap() = true;
    let storage = FihStorage::new(medium.clone(), "walk");
    submit(&storage, "a write the part refuses", "origin/worn");

    let (ids, sizes) = walk(&storage);

    assert_eq!(ids.len(), 1, "the walk lost a refused write: {ids:?}");
    assert_eq!(sizes, vec!["a write the part refuses".len()]);
    assert!(
        medium.map.lock().unwrap().is_empty(),
        "the medium kept a write it had refused"
    );
}
