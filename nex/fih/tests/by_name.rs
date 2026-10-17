// The named reads: one fact, one intent, one hint, and the lookups beside them.
//
// These are the reads a device makes instead of materializing a state: a framing read
// that costs one medium read and no payload, a record read by name, and the inverse
// index from a fact to the intents over it. Each has two sources, the session's maps and
// the medium, and each has to answer the same way when the medium holds a record the
// reader cannot decode.

#![cfg(feature = "std")]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_executor::block_on;
use nex_fih::io::file_io::{FileIo, IoFuture};
use nex_fih::{
    AsyncFactCapable, AsyncHintCapable, AsyncIntentCapable, Content, CoordId, Fact, FactRecord,
    FihStorage, Hint, HintRecord, Intent, IntentRecord,
};

#[derive(Clone, Default)]
struct MemoryIo {
    map: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl MemoryIo {
    fn put(&self, key: &str, data: &[u8]) {
        self.map
            .lock()
            .unwrap()
            .insert(key.to_string(), data.to_vec());
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

fn fact() -> Fact {
    Fact::with_id(
        CoordId::resolve("f_named"),
        "origin/one".into(),
        Content::from("the payload"),
        "agent".into(),
    )
}

fn base_fact(storage: &FihStorage<MemoryIo>) -> Fact {
    let fact = fact();
    block_on(storage.submit_fact(&fact)).expect("the fact is accepted");
    fact
}

#[test]
fn a_named_fact_is_read_from_the_map_or_the_medium() {
    let medium = MemoryIo::default();
    let storage = FihStorage::new(medium.clone(), "by-name");
    let fact = base_fact(&storage);

    let held = block_on(storage.get_fact_framing(&fact.id.to_string())).expect("the map holds it");
    assert_eq!(held.id, fact.id.to_string());
    assert!(
        block_on(storage.get_fact_framing("f_absent")).is_none(),
        "an absent name is absent"
    );

    block_on(storage.flush_pending()).expect("the write reaches the medium");
    let reader = FihStorage::new_without_maps(medium.clone(), "by-name");
    assert!(
        block_on(reader.get_fact_framing(&fact.id.to_string())).is_some(),
        "a store without maps reads the medium"
    );

    // A record the medium holds and the reader cannot decode is reported as absent.
    let corrupted = MemoryIo::default();
    corrupted.put(&FactRecord::fact_key(&fact.id.to_string()), b"not a record");
    let reader = FihStorage::new_without_maps(corrupted, "by-name");
    assert!(block_on(reader.get_fact_framing(&fact.id.to_string())).is_none());
}

#[test]
fn a_named_intent_carries_its_description_only_when_it_has_one() {
    let medium = MemoryIo::default();
    let storage = FihStorage::new(medium.clone(), "by-name");
    let base = base_fact(&storage);
    let with_text = Intent::new(
        CoordId::resolve("i_with_text"),
        vec![base.id],
        None,
        "a description".into(),
        "agent".into(),
    );
    block_on(storage.submit_intent(&with_text)).expect("the intent is accepted");
    let without_text = Intent::new(
        CoordId::resolve("i_without_text"),
        vec![base.id],
        None,
        String::new(),
        "agent".into(),
    );
    block_on(storage.submit_intent(&without_text)).expect("the intent is accepted");

    let (record, description) =
        block_on(storage.get_intent("i_with_text")).expect("the map holds the intent");
    assert_eq!(record.id, with_text.id.to_string());
    assert_eq!(description.as_deref(), Some("a description"));
    assert!(
        block_on(storage.get_intent("i_without_text"))
            .expect("the map holds the intent")
            .1
            .is_none(),
        "an intent without a description reports none"
    );
    assert!(block_on(storage.get_intent("i_absent")).is_none());
    assert!(storage.intent_exists(&with_text.id.to_string()));
    assert!(!storage.intent_exists("i_absent"));

    block_on(storage.flush_pending()).expect("the writes reach the medium");
    let reader = FihStorage::new_without_maps(medium.clone(), "by-name");
    assert!(
        block_on(reader.get_intent("i_with_text"))
            .expect("the medium holds the intent")
            .1
            .as_deref()
            == Some("a description")
    );
    assert!(block_on(reader.get_intent("i_absent")).is_none());

    let corrupted = MemoryIo::default();
    corrupted.put(
        &IntentRecord::intent_key(&with_text.id.to_string()),
        b"not a record",
    );
    let reader = FihStorage::new_without_maps(corrupted, "by-name");
    assert!(block_on(reader.get_intent("i_with_text")).is_none());
}

#[test]
fn a_named_hint_is_read_from_the_map_or_the_medium() {
    let medium = MemoryIo::default();
    let storage = FihStorage::new(medium.clone(), "by-name");
    let hint = Hint {
        id: CoordId::resolve("h_named"),
        content: "what the hint says".into(),
        creator: "agent".into(),
    };
    block_on(storage.submit_hint(&hint)).expect("the hint is accepted");

    let record = block_on(storage.get_hint("h_named")).expect("the map holds the hint");
    assert_eq!(record.content, "what the hint says");
    let (content, creator, _submitted_at) = storage
        .get_hint_by_id(&hint.id.to_string())
        .expect("the map holds the hint");
    assert_eq!(content, "what the hint says");
    assert_eq!(creator, "agent");
    assert!(storage.get_hint_by_id("h_absent").is_none());
    assert!(storage.hint_exists(&hint.id.to_string()));
    assert!(!storage.hint_exists("h_absent"));
    assert!(block_on(storage.get_hint("h_absent")).is_none());

    block_on(storage.flush_pending()).expect("the write reaches the medium");
    let reader = FihStorage::new_without_maps(medium.clone(), "by-name");
    assert!(
        block_on(reader.get_hint("h_named")).is_some(),
        "a store without maps reads the medium"
    );
    assert!(block_on(reader.get_hint("h_absent")).is_none());

    let corrupted = MemoryIo::default();
    corrupted.put(&HintRecord::hint_key(&hint.id.to_string()), b"not a record");
    let reader = FihStorage::new_without_maps(corrupted, "by-name");
    assert!(block_on(reader.get_hint("h_named")).is_none());
}

#[test]
fn the_inverse_index_answers_by_fact_and_keeps_concluded_intents() {
    let storage = FihStorage::new(MemoryIo::default(), "by-name");
    let base = base_fact(&storage);
    for id in ["i_one", "i_two"] {
        let intent = Intent::new(
            CoordId::resolve(id),
            vec![base.id],
            None,
            format!("the intent {id}"),
            "agent".into(),
        );
        block_on(storage.submit_intent(&intent)).expect("the intent is accepted");
    }

    let canonical = base.id.to_string();
    let mut held = storage.intents_by_fact(&canonical);
    held.sort_unstable();
    let mut expected = vec![
        CoordId::resolve("i_one").to_string(),
        CoordId::resolve("i_two").to_string(),
    ];
    expected.sort_unstable();
    assert_eq!(held, expected);
    assert_eq!(
        storage.intents_by_fact("f_named"),
        expected,
        "a label resolves to the canonical fact"
    );
    assert!(storage.intents_by_fact("f_absent").is_empty());

    // A concluded intent still names the fact it came from.
    block_on(storage.claim_intent("i_one", "worker")).expect("the intent is claimed");
    block_on(storage.conclude_intent("i_one", "done")).expect("the intent concludes");
    let mut held = storage.intents_by_fact(&canonical);
    held.sort_unstable();
    assert_eq!(held, expected);
}

#[test]
fn a_semantic_index_is_empty_without_a_registered_store() {
    // An index is assigned when a fact is handed to a semantic store. With none
    // registered there is no index, so a lookup has nothing to resolve.
    let storage = FihStorage::new(MemoryIo::default(), "by-name");
    let fact = Fact::with_id(
        CoordId::resolve("f_one"),
        "origin/one".into(),
        Content::from("one"),
        "agent".into(),
    );
    block_on(storage.submit_fact(&fact)).expect("the fact is accepted");

    assert_eq!(storage.resolve_semantic_idx(0), "");
    assert_eq!(storage.resolve_semantic_idx(99), "");
}
