// The storage surface around the record maps: constructors, the structured read, the
// bounded blob cache, and the fallbacks for a medium that lost a blob.
//
// These are the switches a deployment sets. Auto-flush decides whether a write is durable
// when the call returns, the blob cache cap decides how much content stays in memory, and
// a structured read avoids materializing payloads at all. The fallbacks are what a read
// answers when the medium is missing a blob or holds a legacy record.

#![cfg(feature = "std")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_executor::block_on;
use nex_core::{Now, SystemClock};
use nex_fih::io::file_io::{FileIo, IoFuture};
use nex_fih::{
    AsyncFactCapable, AsyncIntentCapable, AsyncStorageRead, BlackboardError, Content, CoordId,
    Fact, FactRecord, FihStorage, Intent,
};

#[derive(Clone, Default)]
struct MemoryIo {
    map: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    reads: Arc<Mutex<Vec<String>>>,
    refuse: Arc<Mutex<bool>>,
}

impl MemoryIo {
    fn remove(&self, key: &str) {
        self.map.lock().unwrap().remove(key);
    }

    fn put(&self, key: &str, data: &[u8]) {
        self.map
            .lock()
            .unwrap()
            .insert(key.to_string(), data.to_vec());
    }

    fn clear_reads(&self) {
        self.reads.lock().unwrap().clear();
    }

    fn blob_reads(&self) -> usize {
        self.reads
            .lock()
            .unwrap()
            .iter()
            .filter(|path| path.ends_with(".bin") && !path.ends_with(".bin.meta"))
            .count()
    }
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

fn payload_fact(id: &str, payload: &str) -> Fact {
    Fact::with_id(
        CoordId::resolve(id),
        "origin/one".into(),
        Content::from(payload),
        "agent".into(),
    )
}

fn submit(storage: &FihStorage<MemoryIo>, fact: &Fact) {
    block_on(storage.submit_fact(fact)).expect("the fact is accepted");
}

/// A clock pinned to one nanosecond instant, for a device whose epoch is its own boot.
#[derive(Clone, Copy)]
struct FixedClock(u64);

impl Now for FixedClock {
    fn now_nanos(&self) -> u64 {
        self.0
    }

    fn now_secs(&self) -> u64 {
        self.0 / 1_000_000_000
    }
}

#[test]
fn an_auto_flush_store_is_durable_when_the_call_returns() {
    let medium = MemoryIo::default();
    let storage = FihStorage::with_auto_flush(medium.clone(), "surface");
    let fact = payload_fact("f_flush", "payload");
    submit(&storage, &fact);

    let reader = FihStorage::new_without_maps(medium, "surface");
    assert!(
        block_on(reader.get_fact_framing(&fact.id.to_string())).is_some(),
        "the write reached the medium without an explicit flush"
    );
}

#[test]
fn with_all_decides_whether_a_write_is_pending() {
    let medium = MemoryIo::default();
    let storage = FihStorage::with_all(medium.clone(), "surface", Box::new(SystemClock), false);
    let fact = payload_fact("f_pending", "payload");
    submit(&storage, &fact);

    let reader = FihStorage::new_without_maps(medium.clone(), "surface");
    assert!(
        block_on(reader.get_fact_framing(&fact.id.to_string())).is_none(),
        "a non-flushing store keeps the write in memory"
    );

    block_on(storage.flush_pending()).expect("the write reaches the medium");
    assert!(block_on(reader.get_fact_framing(&fact.id.to_string())).is_some());
}

#[test]
fn a_structured_read_omits_payloads_and_descriptions() {
    let storage = FihStorage::new(MemoryIo::default(), "surface");
    let base = payload_fact("f_struct", "the payload");
    submit(&storage, &base);
    for (id, description) in [("i_with_text", "a description"), ("i_without_text", "")] {
        let intent = Intent::new(
            CoordId::resolve(id),
            vec![base.id],
            None,
            description.into(),
            "agent".into(),
        );
        block_on(storage.submit_intent(&intent)).expect("the intent is accepted");
    }

    let state = block_on(storage.read_state_struct());
    assert_eq!(state.facts.len(), 1);
    assert!(
        state.facts[0].content.data.is_empty(),
        "the structured read does not load payloads"
    );
    let with_text = state
        .intents
        .iter()
        .find(|intent| intent.id == CoordId::resolve("i_with_text"))
        .expect("the read holds the intent");
    assert!(with_text.description.is_empty());
    let without_text = state
        .intents
        .iter()
        .find(|intent| intent.id == CoordId::resolve("i_without_text"))
        .expect("the read holds the intent");
    assert_eq!(
        without_text.description,
        CoordId::resolve("i_without_text").to_string(),
        "an intent with no description keeps its identifier"
    );
}

#[test]
fn the_blob_cache_is_bounded_and_reloads_what_it_evicts() {
    let medium = MemoryIo::default();
    let storage = FihStorage::new(medium.clone(), "surface");
    storage.set_blob_cache_cap(1);
    submit(&storage, &payload_fact("f_one", "first payload"));
    submit(&storage, &payload_fact("f_two", "second payload"));
    block_on(storage.flush_pending()).expect("the writes reach the medium");

    block_on(storage.read_state());
    medium.clear_reads();
    block_on(storage.read_state());
    assert_eq!(
        medium.blob_reads(),
        1,
        "the cap of one kept the newer blob and reloaded the older"
    );

    // The cap can be raised at runtime, and a larger cache answers without a read.
    storage.set_blob_cache_cap(100);
    block_on(storage.read_state());
    medium.clear_reads();
    block_on(storage.read_state());
    assert_eq!(medium.blob_reads(), 0, "both blobs stayed in the cache");
}

#[test]
fn a_blob_whose_metadata_is_gone_keeps_its_bytes_and_defaults_the_mime() {
    let medium = MemoryIo::default();
    let storage = FihStorage::new(medium.clone(), "surface");
    let fact = payload_fact("f_meta", "the payload");
    submit(&storage, &fact);
    block_on(storage.flush_pending()).expect("the write reaches the medium");
    let hash = fact.content_hash.to_string();
    medium.remove(&format!("blob/{hash}.bin.meta"));
    drop(storage);

    let reader = FihStorage::new(medium, "surface");
    block_on(reader.rebuild_cache()).expect("the volume reopens");
    let state = block_on(reader.read_state());
    assert_eq!(state.facts[0].content.data, b"the payload");
    assert_eq!(
        state.facts[0].content.mime_type, "application/json",
        "a missing metadata blob falls back to the default mime"
    );
}

#[test]
fn a_blob_whose_payload_is_gone_reads_as_empty() {
    let medium = MemoryIo::default();
    let storage = FihStorage::new(medium.clone(), "surface");
    let fact = payload_fact("f_payload", "the payload");
    submit(&storage, &fact);
    block_on(storage.flush_pending()).expect("the write reaches the medium");
    let hash = fact.content_hash.to_string();
    medium.remove(&format!("blob/{hash}.bin"));
    medium.remove(&format!("blob/{hash}.bin.meta"));
    drop(storage);

    let reader = FihStorage::new(medium, "surface");
    block_on(reader.rebuild_cache()).expect("the volume reopens");
    let state = block_on(reader.read_state());
    assert!(state.facts[0].content.data.is_empty());
    assert_eq!(state.facts[0].content.mime_type, "application/json");
}

#[test]
fn a_legacy_record_hash_is_recomputed_at_open() {
    let medium = MemoryIo::default();
    let payload = "the legacy payload";
    let fact = payload_fact("f_legacy", payload);
    let id = fact.id.to_string();
    let hash = fact.content_hash.to_string();

    // The record names a blob hash that is not hex, and the bytes live under that name.
    // The read at open recomputes the content hash from those bytes.
    let record = FactRecord {
        id: id.clone(),
        blob_hash: "legacy".into(),
        origin: "origin/one".into(),
        creator: "agent".into(),
        submitted_at: 0,
    };
    medium.put(
        &FactRecord::fact_key(&id),
        &postcard::to_allocvec(&record).expect("the record serializes"),
    );
    medium.put("blob/legacy.bin", payload.as_bytes());
    medium.put(&format!("blob/{hash}.bin"), payload.as_bytes());

    let reader = FihStorage::new(medium, "surface");
    block_on(reader.rebuild_cache()).expect("the volume reopens");
    let state = block_on(reader.read_state());
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.facts[0].content.data, payload.as_bytes());
    assert_eq!(state.facts[0].content_hash.to_string(), hash);
}

#[test]
fn a_failed_flush_does_not_stop_a_state_read() {
    let medium = MemoryIo::default();
    let storage = FihStorage::new(medium.clone(), "surface");
    submit(&storage, &payload_fact("f_refused", "the payload"));
    *medium.refuse.lock().unwrap() = true;

    // The read flushes first, and the refusal is logged rather than returned: the maps
    // still hold the record the submission put there.
    let state = block_on(storage.read_state());
    assert_eq!(state.facts.len(), 1);
    assert!(
        state.facts[0].content.data.is_empty(),
        "the refused write kept the payload off the medium"
    );
}

#[test]
fn the_intent_lifecycle_reports_what_each_state_forbids() {
    let storage = FihStorage::with_clock(MemoryIo::default(), "surface", Box::new(SystemClock));

    let empty = Intent::new(
        CoordId::resolve("i_empty"),
        Vec::new(),
        None,
        "an intent with no fact".into(),
        "agent".into(),
    );
    let error = block_on(storage.submit_intent(&empty)).expect_err("an intent needs a fact");
    assert!(matches!(error, BlackboardError::Forbidden(_)), "{error:?}");

    let base = payload_fact("f_base", "base");
    submit(&storage, &base);
    let intent = Intent::new(
        CoordId::resolve("i_state"),
        vec![base.id],
        None,
        "an intent".into(),
        "agent".into(),
    );
    block_on(storage.submit_intent(&intent)).expect("the intent is accepted");
    block_on(storage.claim_intent("i_state", "worker")).expect("the intent is claimed");
    block_on(storage.conclude_intent("i_state", "done")).expect("the intent concludes");

    let error = block_on(storage.claim_intent("i_state", "worker")).expect_err("already concluded");
    assert!(matches!(error, BlackboardError::Internal(_)), "{error:?}");
    let error = block_on(storage.heartbeat("i_state", "worker")).expect_err("already concluded");
    assert!(matches!(error, BlackboardError::Internal(_)), "{error:?}");
    let error =
        block_on(storage.conclude_intent("i_state", "again")).expect_err("already concluded");
    assert!(matches!(error, BlackboardError::Internal(_)), "{error:?}");
}

#[test]
fn a_record_with_no_blob_hash_reads_as_empty() {
    let medium = MemoryIo::default();
    let fact = payload_fact("f_empty_hash", "ignored");
    let id = fact.id.to_string();
    let record = FactRecord {
        id: id.clone(),
        blob_hash: String::new(),
        origin: "origin/one".into(),
        creator: "agent".into(),
        submitted_at: 0,
    };
    medium.put(
        &FactRecord::fact_key(&id),
        &postcard::to_allocvec(&record).expect("the record serializes"),
    );

    let reader = FihStorage::new(medium, "surface");
    block_on(reader.rebuild_cache()).expect("the volume reopens");
    let state = block_on(reader.read_state());
    assert_eq!(state.facts.len(), 1);
    assert!(
        state.facts[0].content.data.is_empty(),
        "a record with no blob has no payload to read"
    );
}

#[test]
fn a_fact_time_below_the_legacy_second_threshold_survives_every_read() {
    // A device that starts its clock at boot writes nanoseconds below 1e12. That is a
    // legitimate nanosecond timestamp, not a pre-convention seconds value.
    let medium = MemoryIo::default();
    let submitted_at = 5_000_000_000; // five seconds since boot
    let writer = FihStorage::with_clock(
        medium.clone(),
        "surface",
        Box::new(FixedClock(submitted_at)),
    );
    let fact = payload_fact("f_boot", "a fact written at five seconds");
    submit(&writer, &fact);
    block_on(writer.flush_pending()).expect("the write reaches the medium");

    // The by-name read on a store without maps.
    let reader = FihStorage::new_without_maps(medium.clone(), "surface");
    let framing = block_on(reader.get_fact_framing(&fact.id.to_string()))
        .expect("the medium holds the record");
    assert_eq!(framing.submitted_at, submitted_at);

    // The walk reads the same record from the same medium.
    let mut walked = None;
    block_on(reader.for_each_fact_record(|record| {
        if record.id == fact.id.to_string() {
            walked = Some(record.submitted_at);
        }
        true
    }))
    .expect("the walk reaches the medium");
    assert_eq!(walked, Some(submitted_at));

    // A reopen through the record maps must not lift it either.
    let keeper = FihStorage::new(medium, "surface");
    block_on(keeper.rebuild_cache()).expect("the maps are built");
    let framed =
        block_on(keeper.get_fact_framing(&fact.id.to_string())).expect("the map holds the record");
    assert_eq!(framed.submitted_at, submitted_at);
}
