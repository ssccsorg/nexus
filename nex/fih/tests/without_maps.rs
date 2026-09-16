// A store that keeps no record maps.
//
// The maps are the application layer: they hold every record the volume contains, at 394 to
// 406 bytes per record on riscv32, and a device with no memory to spare cannot pay it. A
// store made without them holds nothing about the volume. Its writes reach the medium and
// are not remembered, its reads walk the medium, and the checks that would consult a map
// read the medium at the key an identifier implies.
//
// What these tests hold is that the mode is complete: a store without maps writes a volume
// a later session reads, refuses an intent whose fact is absent and accepts one whose fact
// is present, and says so when something asks for an index it cannot build.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_executor::block_on;
use nex_fih::io::file_io::{FileIo, IoFuture};
use nex_fih::{
    AsyncFactCapable, AsyncIntentCapable, AsyncStorageRead, BlackboardError, Content, CoordId,
    Fact, FihStorage, Intent,
};

/// An in-memory medium, shared by every storage that holds a clone of it.
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

fn without_maps(medium: &MemoryIo) -> FihStorage<MemoryIo> {
    FihStorage::new_without_maps(medium.clone(), "without-maps")
}

fn submit(storage: &FihStorage<MemoryIo>, payload: &str) -> CoordId {
    let fact = Fact::new(
        "origin/maps".into(),
        Content::from(payload),
        "writer".into(),
    );
    block_on(storage.submit_fact(&fact)).expect("the fact is accepted")
}

#[test]
fn a_store_without_maps_writes_a_volume_a_later_session_reads() {
    let medium = MemoryIo::default();
    let writer = without_maps(&medium);
    let first = submit(&writer, "one");
    submit(&writer, "two");
    block_on(writer.flush_pending()).expect("the writes reach the medium");

    // A second session, also without maps, which is the device that reboots and asks about
    // a past it wrote.
    let reader = without_maps(&medium);
    let mut walked = Vec::new();
    block_on(reader.for_each_fact(|record, content| {
        walked.push((
            record.id.clone(),
            String::from_utf8_lossy(&content.data).to_string(),
        ));
        true
    }))
    .expect("the walk reaches the medium");

    assert_eq!(
        walked.len(),
        2,
        "the volume holds what was written: {walked:?}"
    );
    assert!(
        walked
            .iter()
            .any(|(id, text)| id == &first.to_string() && text == "one"),
        "the first record came back with its content: {walked:?}"
    );
}

#[test]
fn a_store_without_maps_refuses_an_intent_whose_fact_is_absent() {
    let medium = MemoryIo::default();
    let storage = without_maps(&medium);
    let absent = CoordId::resolve("f_somewhere_not_written");
    let intent = Intent::new(
        CoordId::resolve("i_absent"),
        vec![absent],
        None,
        "an intent over a fact that is not there".to_string(),
        "writer".to_string(),
    );

    match block_on(storage.submit_intent(&intent)) {
        Err(BlackboardError::NotFound(_)) => {}
        other => panic!("an intent over an absent fact was accepted: {other:?}"),
    }
}

#[test]
fn a_store_without_maps_accepts_an_intent_whose_fact_is_on_the_medium() {
    let medium = MemoryIo::default();
    let writer = without_maps(&medium);
    let fact = submit(&writer, "the fact an intent refers to");
    block_on(writer.flush_pending()).expect("the write reaches the medium");

    // The reader has no map, so the check that the fact exists has to come from the
    // medium: the key follows from the identifier.
    let reader = without_maps(&medium);
    let intent = Intent::new(
        CoordId::resolve("i_present"),
        vec![fact],
        None,
        "an intent over a fact that is there".to_string(),
        "writer".to_string(),
    );
    block_on(reader.submit_intent(&intent)).expect("the intent is accepted");
}

#[test]
fn rebuilding_the_maps_makes_a_store_that_keeps_them() {
    let medium = MemoryIo::default();
    let writer = without_maps(&medium);
    submit(&writer, "one");
    submit(&writer, "two");
    block_on(writer.flush_pending()).expect("the writes reach the medium");

    // The same store, asked to rebuild: the maps are what the rebuild is for, so a store
    // that had none has them after it.
    let storage = without_maps(&medium);
    block_on(storage.rebuild_cache()).expect("the maps are built");
    let state = block_on(storage.read_state());
    assert_eq!(state.facts.len(), 2, "the rebuilt store holds the volume");
}

#[test]
fn a_store_without_maps_has_nothing_to_index() {
    let medium = MemoryIo::default();
    let storage = without_maps(&medium);
    submit(&storage, "one");
    block_on(storage.flush_pending()).expect("the write reaches the medium");

    let outcome = block_on(storage.rebuild_semantic());
    assert!(
        outcome.is_err(),
        "a store without maps built an index of nothing: {outcome:?}"
    );
}
