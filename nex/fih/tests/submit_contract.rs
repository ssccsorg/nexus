// What a fact submission does when the address it is given is already taken.
//
// A fact's id is derived from content, origin, and creator, so a caller that supplies the
// id itself can point two contents at one address. The submit path promises that the
// first record is not overwritten: an identical retry is accepted, and a different
// content is a conflict. The guard has to hold in three places, because the earlier
// record can be in the session's map, in its pending buffer, or only on the medium.

#![cfg(feature = "std")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_executor::block_on;
use nex_fih::io::file_io::{FileIo, IoFuture};
use nex_fih::{
    AsyncFactCapable, AsyncStorageRead, BlackboardError, Content, CoordId, Fact, FihStorage,
};

/// An in-memory medium shared by every storage that holds a clone of it.
#[derive(Clone, Default)]
struct MemoryIo {
    map: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl MemoryIo {
    fn keys(&self, prefix: &str) -> Vec<String> {
        let mut keys: Vec<String> = self
            .map
            .lock()
            .unwrap()
            .keys()
            .filter(|key| key.starts_with(prefix))
            .cloned()
            .collect();
        keys.sort();
        keys
    }
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

/// A fact at a caller-chosen address.
fn at(id: &str, payload: &str) -> Fact {
    Fact::with_id(
        CoordId::resolve(id),
        "origin/one".into(),
        Content::from(payload),
        "agent".into(),
    )
}

fn submit(storage: &FihStorage<MemoryIo>, fact: &Fact) -> Result<CoordId, BlackboardError> {
    block_on(storage.submit_fact(fact))
}

#[test]
fn an_identical_retry_keeps_one_record_and_one_blob() {
    let medium = MemoryIo::default();
    let storage = FihStorage::new(medium.clone(), "submit");
    let fact = at("f_taken", "the payload");

    submit(&storage, &fact).expect("the fact is accepted");
    submit(&storage, &fact).expect("the retry is idempotent");
    block_on(storage.flush_pending()).expect("the write reaches the medium");

    assert_eq!(block_on(storage.read_state()).facts.len(), 1);
    assert_eq!(
        medium.keys("blob/").len(),
        2,
        "the payload and its metadata are written once"
    );
}

#[test]
fn a_second_content_at_a_taken_id_is_a_conflict_and_keeps_the_first() {
    let medium = MemoryIo::default();
    let storage = FihStorage::new(medium.clone(), "submit");

    submit(&storage, &at("f_taken", "the first content")).expect("the first fact is accepted");
    let error = submit(&storage, &at("f_taken", "the second content"))
        .expect_err("a different content at the same id is refused");
    match error {
        BlackboardError::Conflict(message) => {
            assert!(
                message.contains(&CoordId::resolve("f_taken").to_string()),
                "the conflict names the address: {message}"
            );
            assert!(message.contains("different content_hash"), "{message}");
        }
        other => panic!("the conflict landed in another category: {other:?}"),
    }

    block_on(storage.flush_pending()).expect("the write reaches the medium");
    let state = block_on(storage.read_state());
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.facts[0].content.data, b"the first content");

    // The refused submission enqueued nothing, so a reopen sees the same volume.
    drop(storage);
    let reader = FihStorage::new(medium, "submit");
    block_on(reader.rebuild_cache()).expect("the volume reopens");
    let state = block_on(reader.read_state());
    assert_eq!(state.facts[0].content.data, b"the first content");
}

#[test]
fn a_pending_record_is_a_taken_id_for_a_store_that_keeps_no_maps() {
    let storage = FihStorage::new_without_maps(MemoryIo::default(), "submit");

    submit(&storage, &at("f_taken", "the first content")).expect("the first fact is accepted");
    submit(&storage, &at("f_taken", "the first content")).expect("the retry is idempotent");
    let error = submit(&storage, &at("f_taken", "the second content"))
        .expect_err("the pending record is the earlier one");

    assert!(matches!(error, BlackboardError::Conflict(_)), "{error:?}");
}

#[test]
fn a_volume_another_session_wrote_reports_the_conflict_from_the_medium() {
    let medium = MemoryIo::default();
    let writer = FihStorage::new(medium.clone(), "submit");
    submit(&writer, &at("f_taken", "the first content")).expect("the first fact is accepted");
    block_on(writer.flush_pending()).expect("the write reaches the medium");
    drop(writer);

    // The reader holds no maps and has read nothing, so the medium is the only place
    // that can answer whether the address is taken.
    let reader = FihStorage::new(medium.clone(), "submit");
    let error = submit(&reader, &at("f_taken", "the second content"))
        .expect_err("the medium's record is the earlier one");
    assert!(matches!(error, BlackboardError::Conflict(_)), "{error:?}");

    block_on(reader.rebuild_cache()).expect("the volume reopens");
    let state = block_on(reader.read_state());
    assert_eq!(state.facts[0].content.data, b"the first content");
}

#[test]
fn a_different_identity_at_a_colliding_address_is_a_conflict() {
    let medium = MemoryIo::default();
    let storage = FihStorage::new(medium, "submit");

    // The digest is taken over concatenated fields, so these two identities reach one
    // address. The store must keep the first and refuse the second rather than accept
    // the second as an idempotent retry of the first.
    let first = Fact::new("ab".into(), Content::from("the same payload"), "c".into());
    let shifted = Fact::with_id(
        first.id,
        "a".into(),
        Content::from("the same payload"),
        "bc".into(),
    );
    assert_eq!(first.id, shifted.id, "the addresses collide");

    submit(&storage, &first).expect("the first fact is accepted");
    let error = submit(&storage, &shifted).expect_err("the second identity is refused");
    assert!(matches!(error, BlackboardError::Conflict(_)), "{error:?}");

    let state = block_on(storage.read_state());
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.facts[0].origin, "ab");
    assert_eq!(state.facts[0].creator, "c");
}
