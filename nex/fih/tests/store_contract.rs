// The storage contract, held to its behaviour.
//
// The traits in `nex_fih::contract::store` are the port a fabric exposes, and a second
// implementation has to be able to take this store's place behind it. The check below is
// written against the traits alone: it names no concrete store, so it fails to compile if
// the contract is missing an operation a consumer needs, and it fails at run time if an
// implementation answers differently.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_executor::block_on;
use nex_fih::io::file_io::{FileIo, IoFuture};
use nex_fih::{
    AsyncFactCapable, Content, Fact, FactById, FactRecord, FactWalk, FihStorage, StoreMaintenance,
};

/// An in-memory medium, shared by every store that holds a clone of it.
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

/// The behaviour every implementation of the contract owes a consumer.
///
/// Nothing here names a store type, which is the point: an implementation that satisfies
/// the traits can be substituted without touching this function.
async fn conformance<S>(store: &S, ids: &[String])
where
    S: FactById + FactWalk + StoreMaintenance,
{
    store
        .rebuild()
        .await
        .expect("a reader opens the store before reading by identity");

    for id in ids {
        let read = store.fact_by_id(id).await;
        assert!(
            read.is_some(),
            "a record the store holds is read by id: {id}"
        );

        let framing = store.fact_framing(id).await.expect("framing is readable");
        assert_eq!(&framing.id, id, "framing names the record asked for");
    }

    assert!(
        store.fact_by_id("no-such-record").await.is_none(),
        "an absent record is absent rather than an error"
    );

    let mut walked: Vec<String> = Vec::new();
    store
        .for_each_fact(|record: &FactRecord, content: Content| {
            assert!(
                !content.data.is_empty(),
                "the content walk hands over the payload of {}",
                record.id
            );
            walked.push(record.id.clone());
            true
        })
        .await
        .expect("the walk reaches the store");
    assert_eq!(
        walked, ids,
        "the walk visits every record, in the store's order"
    );

    let mut framed: Vec<String> = Vec::new();
    store
        .for_each_fact_record(|record: &FactRecord| {
            framed.push(record.id.clone());
            true
        })
        .await
        .expect("the framing walk reaches the store");
    assert_eq!(framed, ids, "the framing walk visits the same set");

    let mut stopped = 0;
    store
        .for_each_fact_record(|_: &FactRecord| {
            stopped += 1;
            false
        })
        .await
        .expect("a walk that stops early still returns");
    assert_eq!(stopped, 1, "the visitor's stop ends the walk");

    store.flush().await.expect("a session settles");
}

#[test]
fn the_contract_answers_a_read_a_walk_and_maintenance() {
    let store = FihStorage::new(MemoryIo::default(), "contract");

    for payload in ["first", "second", "third"] {
        let fact = Fact::new(
            "origin/contract".into(),
            Content::from(payload),
            "writer".into(),
        );
        block_on(store.submit_fact(&fact)).expect("the fact is accepted");
    }
    block_on(store.flush_pending()).expect("the writes reach the medium");

    // The identifiers a consumer would hold: taken through the walk, so the check reads
    // records it did not name itself.
    let mut ids: Vec<String> = Vec::new();
    block_on(store.for_each_fact_record(|record: &FactRecord| {
        ids.push(record.id.clone());
        true
    }))
    .expect("the store enumerates");
    assert_eq!(ids.len(), 3, "three facts were submitted: {ids:?}");
    ids.sort();

    block_on(conformance(&store, &ids));
}

#[test]
fn the_contract_survives_a_second_session_over_the_same_volume() {
    let medium = MemoryIo::default();
    let writer = FihStorage::new(medium.clone(), "contract");
    let fact = Fact::new(
        "origin/contract".into(),
        Content::from("written by the first session"),
        "writer".into(),
    );
    block_on(writer.submit_fact(&fact)).expect("the fact is accepted");
    block_on(writer.flush_pending()).expect("the writes reach the medium");

    // A second session that has read nothing yet: the device asked about a past it did not
    // write, which is the case the read contract exists for.
    let reader = FihStorage::new(medium, "contract");
    let mut ids: Vec<String> = Vec::new();
    block_on(reader.for_each_fact_record(|record: &FactRecord| {
        ids.push(record.id.clone());
        true
    }))
    .expect("the second session enumerates");
    assert_eq!(ids.len(), 1, "the second session sees the record");

    block_on(conformance(&reader, &ids));
}
