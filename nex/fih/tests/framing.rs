// Reading one named record's framing.
//
// A record's address is computed from what it holds, so a reader that has the identifier has the
// key, and reaching the record is one medium read rather than a walk of the volume. What a framing
// read answers is what the record says about itself: its identifier, its origin, its creator, and
// when it was written. The content is a second read that a caller asking about the framing does not
// make, which on a device is the budget the read does not spend. What these tests hold is the
// framing that comes back, the payload that was not asked for, and the two modes the read has, a
// store that holds the record maps and a store that keeps none, agreeing about the same record.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_executor::block_on;
use nex_fih::io::file_io::{FileIo, IoFuture};
use nex_fih::{AsyncFactCapable, Content, CoordId, Fact, FihStorage};

/// An in-memory medium, shared by every storage that holds a clone of it, that keeps the keys
/// every read asked for. The log is how a test says what a read did not ask for, which is the half
/// of this read that its cost is about.
#[derive(Clone, Default)]
struct MemoryIo {
    map: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    reads: Arc<Mutex<Vec<String>>>,
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
        Box::pin(async move {
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

fn submit(storage: &FihStorage<MemoryIo>, payload: &str, origin: &str) -> CoordId {
    let fact = Fact::new(origin.into(), Content::from(payload), "writer".into());
    block_on(storage.submit_fact(&fact)).expect("the fact is accepted")
}

#[test]
fn a_named_record_s_framing_comes_back_without_a_payload_read() {
    let medium = MemoryIo::default();
    let writer = FihStorage::new(medium.clone(), "framing");
    let id = submit(
        &writer,
        "the payload a framing read does not ask for",
        "origin/framing",
    );
    block_on(writer.flush_pending()).expect("the write reaches the medium");

    // A store that keeps no map, which is the device asking about a record it did not write: the
    // identifier is the key, so the read reaches the record and stops there.
    let reader = FihStorage::new_without_maps(medium.clone(), "framing");
    let before = medium.reads.lock().unwrap().len();
    let framing =
        block_on(reader.get_fact_framing(&id.to_string())).expect("the volume holds the record");

    assert_eq!(framing.id, id.to_string(), "the record asked for came back");
    assert_eq!(framing.origin, "origin/framing");
    assert_eq!(framing.creator, "writer");
    assert!(framing.submitted_at > 0, "the record carries its own time");

    // The cost, which is what the read exists for: one key, and that key is the record rather than
    // a payload. A read that materialized the content would have asked the medium for the blob and
    // its metadata as well, which is what a walk of the volume pays per record it passes.
    let asked: Vec<String> = medium.reads.lock().unwrap()[before..].to_vec();
    assert_eq!(asked.len(), 1, "a framing read is one read: {asked:?}");
    assert!(
        !asked.iter().any(|key| key.contains("blob/")),
        "a framing read asked for a payload: {asked:?}"
    );

    // The pair of reads, which is what keeping one lookup in one place is for: the read that
    // answers the content agrees with this one about the record it reached.
    let with_content =
        block_on(reader.get_fact_by_id(&id.to_string())).expect("the volume holds the record");
    assert_eq!(with_content.2, framing.origin);
    assert_eq!(with_content.3, framing.creator);
}

/// The two modes of one read, over one volume: the host that holds the record maps and the device
/// that keeps none. A difference here would make the two disagree about a record rather than about
/// how it was reached.
#[test]
fn a_store_with_maps_and_a_store_without_them_answer_the_same_framing() {
    let medium = MemoryIo::default();
    let writer = FihStorage::new(medium.clone(), "framing");
    let first = submit(&writer, "one", "origin/framing");
    let second = submit(&writer, "two", "origin/other");
    block_on(writer.flush_pending()).expect("the writes reach the medium");

    let with_maps = FihStorage::new(medium.clone(), "framing");
    block_on(with_maps.rebuild_cache()).expect("the maps are built");
    let without_maps = FihStorage::new_without_maps(medium, "framing");

    for id in [&first, &second] {
        let held = block_on(with_maps.get_fact_framing(&id.to_string()));
        let walked = block_on(without_maps.get_fact_framing(&id.to_string()));
        assert!(held.is_some(), "the volume holds {id}");
        assert_eq!(held, walked, "the two modes answer one framing for {id}");
    }
}

#[test]
fn an_identifier_the_volume_does_not_hold_is_no_framing() {
    let medium = MemoryIo::default();
    let writer = FihStorage::new(medium.clone(), "framing");
    block_on(writer.flush_pending()).expect("the medium is settled");

    // An absent record is an empty answer rather than a failed read, which is what the read that
    // answers the content reports for the same name.
    let reader = FihStorage::new_without_maps(medium, "framing");
    assert!(
        block_on(reader.get_fact_framing("f_not_written_anywhere")).is_none(),
        "an absent record answered a framing"
    );
}
