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
    AsyncFactCapable, AsyncHintCapable, AsyncIntentCapable, AsyncStorageRead, BlackboardError,
    Content, CoordId, Fact, FihStorage, Hint, Intent,
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

/// The mode's own invariant, held where the mode is defined rather than where its footprint
/// is measured: a store without maps holds nothing about the volume it writes, which is what
/// keeps the memory a product needs independent of how many records it keeps. The footprint
/// consequence is measured on the device tier in ktema; this is the half that fails first,
/// because a path that starts remembering shows up as a map with entries in it.
#[test]
fn a_store_without_maps_keeps_nothing_about_the_volume() {
    let medium = MemoryIo::default();
    let storage = without_maps(&medium);
    let fact = submit(&storage, "a fact the store must not keep");
    submit(&storage, "a second one");
    block_on(storage.flush_pending()).expect("the writes reach the medium");

    // An intent and a hint, because the three record kinds reach their maps through
    // different paths and the mode has to hold for all of them.
    block_on(storage.submit_intent(&Intent::new(
        CoordId::resolve("i_over_that_fact"),
        vec![fact],
        None,
        "an intent over the fact".to_string(),
        "writer".to_string(),
    )))
    .expect("the intent is accepted");
    block_on(storage.submit_hint(&Hint {
        id: CoordId::resolve("h_of_the_volume"),
        content: "a hint the store must not keep".to_string(),
        creator: "writer".to_string(),
    }))
    .expect("the hint is accepted");
    block_on(storage.flush_pending()).expect("the rest reaches the medium");

    // An intent that has run to its end. Concluding writes a conclusion fact and places it
    // through a path of its own, which has to keep nothing either: the record reaches the
    // medium through the pending buffer, and the map is not what makes it readable.
    block_on(storage.claim_intent("i_over_that_fact", "writer")).expect("the intent is claimed");
    block_on(storage.conclude_intent("i_over_that_fact", "the result"))
        .expect("the intent is concluded");
    block_on(storage.flush_pending()).expect("the conclusion reaches the medium");

    for (kind, kept) in [
        ("fact", storage.fact_records.borrow().len()),
        ("intent", storage.intent_records.borrow().len()),
        ("hint", storage.hint_records.borrow().len()),
    ] {
        assert_eq!(kept, 0, "a store without maps kept {kept} {kind} records");
    }
}
