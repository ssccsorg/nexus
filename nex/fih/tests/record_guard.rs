// The conflict guard across sessions.
//
// `submit_fact` refuses a second fact at the same identifier unless it carries the
// identical content: the identifier is a content address, so a different hash at the same
// id means the address is not safe and the earlier record must not be written over. The
// check reads the record layer's map, and a device that has just opened a volume has no
// map: it is reading a volume it wrote before it rebooted. The medium answers the check
// instead, at the key the identifier implies, which is what a computed address is for.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_executor::block_on;
use nex_fih::io::file_io::{FileIo, IoFuture};
use nex_fih::{AsyncFactCapable, BlackboardError, Content, Fact, FihStorage};

/// An in-memory medium that keeps what was asked of it.
#[derive(Clone, Default)]
struct MemoryIo {
    map: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    writes: Arc<Mutex<Vec<String>>>,
}

impl FileIo for MemoryIo {
    fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        let map = Arc::clone(&self.map);
        Box::pin(async move { Ok(map.lock().unwrap().get(path).cloned()) })
    }

    fn write<'a>(&'a self, path: &'a str, data: &'a [u8]) -> IoFuture<'a, ()> {
        let map = Arc::clone(&self.map);
        let writes = Arc::clone(&self.writes);
        Box::pin(async move {
            writes.lock().unwrap().push(path.to_string());
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

/// The fact records that reached the medium, which is the number a retry must not add to.
fn fact_writes(medium: &MemoryIo) -> Vec<String> {
    medium
        .writes
        .lock()
        .unwrap()
        .iter()
        .filter(|key| key.starts_with("facts/"))
        .cloned()
        .collect()
}

fn stored(medium: &MemoryIo) -> Fact {
    let fact = Fact::new(
        "origin/guard".into(),
        Content::from("the payload"),
        "writer".into(),
    );
    let writer = FihStorage::new(medium.clone(), "guard");
    // This is the guard's first case, and it has to hold for the tests below to mean
    // anything: the record reaches the medium.
    block_on(writer.submit_fact(&fact)).expect("the first write is accepted");
    block_on(writer.flush_pending()).expect("the write reaches the medium");
    assert_eq!(fact_writes(medium).len(), 1, "the record was written");
    fact
}

#[test]
fn a_session_that_did_not_write_the_volume_recognizes_the_record() {
    let medium = MemoryIo::default();
    let fact = stored(&medium);

    // A second session over the same medium. Nothing has read the volume into memory, so
    // the map cannot answer and the medium has to.
    let next = FihStorage::new(medium.clone(), "guard");
    let again = block_on(next.submit_fact(&fact)).expect("the retry is accepted");
    block_on(next.flush_pending()).expect("there is nothing to flush");

    assert_eq!(again.to_string(), fact.id.to_string());
    assert_eq!(
        fact_writes(&medium).len(),
        1,
        "the retry wrote the record a second time"
    );
}

#[test]
fn a_session_that_did_not_write_the_volume_refuses_a_different_content() {
    let medium = MemoryIo::default();
    let fact = stored(&medium);

    // The same identifier carrying other content: the address is not safe, so the record
    // the medium holds must stand.
    let forged = Fact::with_id(
        fact.id,
        fact.origin.clone(),
        Content::from("another payload"),
        fact.creator.clone(),
    );
    let next = FihStorage::new(medium.clone(), "guard");
    match block_on(next.submit_fact(&forged)) {
        Err(BlackboardError::Conflict(message)) => {
            assert!(
                message.contains(&fact.id.to_string()),
                "the refusal does not name the identifier: {message}"
            );
        }
        other => panic!("a conflicting identifier was accepted: {other:?}"),
    }
    assert_eq!(
        fact_writes(&medium).len(),
        1,
        "the refused write reached the medium"
    );
}
