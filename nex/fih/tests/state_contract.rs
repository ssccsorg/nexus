// The state network's read surface, held to its behaviour.
//
// The traits in `nex_fih::contract::state` are what a network answers without saying where
// the records live. The check below is written against the traits alone: it names no concrete
// network, so it fails to compile if the surface is missing an operation a consumer needs, and
// it fails at run time if an implementation answers differently.
//
// Holding the records is the caller's business. The setup here uses the concrete network
// because writing, flushing, and loading are operations of a layer that implements persistence,
// and that layer is what the read surface is read through.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_executor::block_on;
use nex_fih::io::file_io::{FileIo, IoFuture};
use nex_fih::{AsyncFactCapable, Content, Fact, FactById, FactRecord, FactWalk, FihStorage};

/// An in-memory medium, shared by every network that holds a clone of it.
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

/// What every network owes a reader of its state.
///
/// Nothing here names a network type, which is the point: an implementation that satisfies the
/// traits answers the same way, whatever holds the records.
async fn read_surface<S>(network: &S, ids: &[String])
where
    S: FactById + FactWalk,
{
    for id in ids {
        let read = network.fact_by_id(id).await;
        assert!(
            read.is_some(),
            "a record the network holds is read by id: {id}"
        );

        let framing = network.fact_framing(id).await.expect("framing is readable");
        assert_eq!(&framing.id, id, "framing names the record asked for");
    }

    assert!(
        network.fact_by_id("no-such-record").await.is_none(),
        "an absent record is absent rather than an error"
    );

    let mut walked: Vec<String> = Vec::new();
    network
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
        .expect("the walk reaches the records");
    assert_eq!(
        walked, ids,
        "the walk visits every record, in the network's order"
    );

    let mut framed: Vec<String> = Vec::new();
    network
        .for_each_fact_record(|record: &FactRecord| {
            framed.push(record.id.clone());
            true
        })
        .await
        .expect("the framing walk reaches the records");
    assert_eq!(framed, ids, "the framing walk visits the same set");

    let mut stopped = 0;
    network
        .for_each_fact_record(|_: &FactRecord| {
            stopped += 1;
            false
        })
        .await
        .expect("a walk that stops early still returns");
    assert_eq!(stopped, 1, "the visitor's stop ends the walk");
}

#[test]
fn the_read_surface_answers_over_a_held_volume() {
    let network = FihStorage::new(MemoryIo::default(), "contract");

    for payload in ["first", "second", "third"] {
        let fact = Fact::new(
            "origin/contract".into(),
            Content::from(payload),
            "writer".into(),
        );
        block_on(network.submit_fact(&fact)).expect("the fact is accepted");
    }
    block_on(network.flush_pending()).expect("the writes reach the medium");

    // The identifiers a consumer would hold, taken through the walk so the check reads
    // records it did not name itself.
    let mut ids: Vec<String> = Vec::new();
    block_on(network.for_each_fact_record(|record: &FactRecord| {
        ids.push(record.id.clone());
        true
    }))
    .expect("the network enumerates");
    assert_eq!(ids.len(), 3, "three facts were submitted: {ids:?}");
    ids.sort();

    block_on(read_surface(&network, &ids));
}

#[test]
fn the_read_surface_answers_a_network_another_session_wrote() {
    let medium = MemoryIo::default();
    let writer = FihStorage::new(medium.clone(), "contract");
    let fact = Fact::new(
        "origin/contract".into(),
        Content::from("written by the first session"),
        "writer".into(),
    );
    block_on(writer.submit_fact(&fact)).expect("the fact is accepted");
    block_on(writer.flush_pending()).expect("the writes reach the medium");

    // A network that holds no map reads the medium at the key the identifier implies, so a
    // session that read nothing yet answers for a volume it did not write.
    let reader = FihStorage::new_without_maps(medium, "contract");
    let mut ids: Vec<String> = Vec::new();
    block_on(reader.for_each_fact_record(|record: &FactRecord| {
        ids.push(record.id.clone());
        true
    }))
    .expect("the second session enumerates");
    assert_eq!(ids.len(), 1, "the second session sees the record");

    block_on(read_surface(&reader, &ids));
}
