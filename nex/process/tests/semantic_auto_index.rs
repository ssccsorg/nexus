// What the engine hands a semantic store when it auto-indexes a submitted fact.
//
// A store is written against `RecordLoad`, and the two accessors answer different
// questions: `content` is the bytes the engine already holds, and `features` is an
// embedding the engine does not compute. A fact therefore arrives as text with no
// features, and a store that needs a vector has to build it itself. Conclusion facts are
// skipped, because they are the process layer's own output rather than an observation.

use std::sync::{Arc, Mutex};

use futures_executor::block_on;
use nex::FihStorage;
use nex::storage::semantic::{Query, RecordLoad, SemanticStore};
use nex_fih::{AsyncFactCapable, AsyncIntentCapable, Content, CoordId, Fact};
use nexus_storage_sim::SimIo;

#[derive(Debug, Clone, Default)]
struct Recorder(Arc<Mutex<Vec<Call>>>);

#[derive(Debug, Clone)]
struct Call {
    id: u32,
    text: Option<Vec<u8>>,
    features: Option<Vec<f32>>,
}

#[async_trait::async_trait]
impl SemanticStore for Recorder {
    async fn insert(&mut self, id: u32, load: &dyn RecordLoad) -> Result<(), String> {
        self.0.lock().unwrap().push(Call {
            id,
            text: load.content(id),
            features: load.features(id),
        });
        Ok(())
    }

    async fn search(&self, _query: &dyn Query, _top_k: usize) -> Result<Vec<(u32, f32)>, String> {
        Ok(Vec::new())
    }

    async fn remove(&mut self, _id: u32) -> Result<(), String> {
        Ok(())
    }

    fn len(&self) -> usize {
        self.0.lock().unwrap().len()
    }
}

fn storage_with(recorder: &Recorder) -> FihStorage<SimIo> {
    let storage = FihStorage::new(SimIo::new(), "auto-index");
    storage.register_semantic_store(Box::new(recorder.clone()));
    storage
}

#[test]
fn a_submitted_fact_reaches_a_semantic_store_as_text_without_features() {
    let recorder = Recorder::default();
    let storage = storage_with(&recorder);

    let fact = Fact::with_id(
        CoordId::resolve("f_indexed"),
        "origin/one".into(),
        Content::from("the indexed text"),
        "agent".into(),
    );
    block_on(storage.submit_fact(&fact)).expect("the fact is accepted");

    assert!(
        !storage.semantic_stores()[0].is_empty(),
        "the default is_empty reads the store's own length"
    );
    let calls = recorder.0.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].text.as_deref(),
        Some(b"the indexed text".as_slice()),
        "the store receives the content the engine holds"
    );
    assert!(
        calls[0].features.is_none(),
        "the engine does not compute an embedding"
    );
}

#[test]
fn a_conclusion_fact_is_not_auto_indexed() {
    let recorder = Recorder::default();
    let storage = storage_with(&recorder);

    // A fact whose origin names the conclusion namespace is the process layer's own
    // output, so indexing it would feed the store what an agent did not observe.
    let conclusion = Fact::with_id(
        CoordId::resolve("f_concl_manual"),
        "conclusion:i_manual".into(),
        Content::from("a conclusion"),
        "agent".into(),
    );
    block_on(storage.submit_fact(&conclusion)).expect("the fact is accepted");

    assert!(recorder.0.lock().unwrap().is_empty());
}

#[test]
fn rebuilding_the_semantic_index_hands_text_and_no_features_again() {
    let recorder = Recorder::default();
    let storage = storage_with(&recorder);
    for (id, text) in [("f_one", "first text"), ("f_two", "second text")] {
        let fact = Fact::with_id(
            CoordId::resolve(id),
            "origin/one".into(),
            Content::from(text),
            "agent".into(),
        );
        block_on(storage.submit_fact(&fact)).expect("the fact is accepted");
    }
    block_on(storage.flush_pending()).expect("the writes reach the medium");
    recorder.0.lock().unwrap().clear();

    block_on(storage.rebuild_semantic()).expect("the index rebuilds");

    let calls = recorder.0.lock().unwrap();
    assert_eq!(calls.len(), 2, "both records reach the store again");
    assert!(
        calls.iter().all(|call| call.features.is_none()),
        "the rebuild does not invent embeddings"
    );
    let mut texts: Vec<String> = calls
        .iter()
        .map(|call| String::from_utf8_lossy(call.text.as_deref().unwrap_or_default()).to_string())
        .collect();
    texts.sort();
    assert_eq!(
        texts,
        vec!["first text".to_string(), "second text".to_string()]
    );
}

#[test]
fn rebuilding_the_index_needs_maps_and_a_registered_store() {
    let no_maps = FihStorage::new_without_maps(SimIo::new(), "auto-index");
    let error = block_on(no_maps.rebuild_semantic())
        .expect_err("a store that keeps no maps has nothing to index");
    assert!(error.contains("record maps"), "{error}");

    let empty = FihStorage::new(SimIo::new(), "auto-index");
    block_on(empty.rebuild_semantic()).expect("with no stores the rebuild is a no-op");
}

#[test]
fn the_conclude_path_writes_the_fact_without_indexing_it() {
    let recorder = Recorder::default();
    let storage = storage_with(&recorder);

    let base = Fact::with_id(
        CoordId::resolve("f_base"),
        "origin/one".into(),
        Content::from("base"),
        "agent".into(),
    );
    block_on(storage.submit_fact(&base)).expect("the base fact is accepted");

    let intent = nex_fih::Intent::new(
        CoordId::resolve("i_indexed"),
        vec![base.id],
        None,
        "an intent".into(),
        "agent".into(),
    );
    block_on(storage.submit_intent(&intent)).expect("the intent is accepted");
    block_on(storage.claim_intent("i_indexed", "worker")).expect("the intent is claimed");
    block_on(storage.conclude_intent("i_indexed", "resolved")).expect("the intent concludes");

    // Only the base fact was indexed; the conclusion is written to the storage alone.
    assert_eq!(recorder.0.lock().unwrap().len(), 1);
}

#[derive(Debug)]
struct Rejecting;

#[async_trait::async_trait]
impl SemanticStore for Rejecting {
    async fn insert(&mut self, _id: u32, _load: &dyn RecordLoad) -> Result<(), String> {
        Err("no features available".into())
    }

    async fn search(&self, _query: &dyn Query, _top_k: usize) -> Result<Vec<(u32, f32)>, String> {
        Ok(Vec::new())
    }

    async fn remove(&mut self, _id: u32) -> Result<(), String> {
        Ok(())
    }

    fn len(&self) -> usize {
        0
    }
}

struct TextLoad;

impl RecordLoad for TextLoad {
    fn content(&self, _id: u32) -> Option<Vec<u8>> {
        Some(b"text".to_vec())
    }

    fn features(&self, _id: u32) -> Option<Vec<f32>> {
        None
    }
}

struct TextQuery;

impl Query for TextQuery {
    fn features(&self) -> Option<Vec<f32>> {
        None
    }

    fn text(&self) -> Option<String> {
        Some("text".into())
    }
}

#[test]
fn a_store_that_refuses_an_insert_reports_the_error_and_stays_registered() {
    let storage = FihStorage::new(SimIo::new(), "auto-index");
    storage.register_semantic_store(Box::new(Rejecting));

    let error =
        block_on(storage.semantic_insert(0, &TextLoad)).expect_err("the store refuses the record");
    assert_eq!(error, "no features available");
    assert_eq!(
        TextLoad.text(0).as_deref(),
        Some("text"),
        "text defaults to the content as UTF-8"
    );
    assert_eq!(
        storage.semantic_stores().len(),
        1,
        "the store is put back after the failure"
    );
}

#[test]
fn a_search_with_no_store_says_so_and_one_with_a_store_answers() {
    let empty = FihStorage::new(SimIo::new(), "auto-index");
    let error = block_on(empty.semantic_search(&TextQuery, 5)).expect_err("nothing to search");
    assert!(error.contains("no semantic stores"), "{error}");

    let recorder = Recorder::default();
    let storage = storage_with(&recorder);
    let results = block_on(storage.semantic_search(&TextQuery, 5)).expect("the store answers");
    assert!(results.is_empty(), "the recorder has no matches");
}

#[test]
fn a_rebuilt_index_resolves_each_id_to_the_record_it_indexed() {
    let recorder = Recorder::default();
    let storage = storage_with(&recorder);
    let mut held: Vec<(String, String)> = Vec::new();
    for id in ["f_one", "f_two"] {
        let fact = Fact::with_id(
            CoordId::resolve(id),
            "origin/one".into(),
            Content::from(id),
            "agent".into(),
        );
        block_on(storage.submit_fact(&fact)).expect("the fact is accepted");
        held.push((fact.id.to_string(), id.to_string()));
    }
    block_on(storage.flush_pending()).expect("the writes reach the medium");
    recorder.0.lock().unwrap().clear();

    block_on(storage.rebuild_semantic()).expect("the index rebuilds");

    let calls = recorder.0.lock().unwrap();
    assert_eq!(calls.len(), 2);
    for call in calls.iter() {
        let resolved = storage.resolve_semantic_idx(call.id);
        let expected = held
            .iter()
            .find(|(id, _)| *id == resolved)
            .map(|(_, text)| text.as_bytes());
        assert_eq!(
            call.text.as_deref(),
            expected,
            "index {} did not resolve to the record it indexed",
            call.id
        );
    }
}

#[test]
fn a_rebuild_skips_a_fact_with_no_readable_text() {
    let recorder = Recorder::default();
    let storage = storage_with(&recorder);

    let whitespace = Fact::with_id(
        CoordId::resolve("f_whitespace"),
        "origin/one".into(),
        Content::from("   "),
        "agent".into(),
    );
    let missing_blob = Fact::with_id(
        CoordId::resolve("f_missing_blob"),
        "origin/one".into(),
        Content::from("the payload"),
        "agent".into(),
    );
    block_on(storage.submit_fact(&whitespace)).expect("the fact is accepted");
    block_on(storage.submit_fact(&missing_blob)).expect("the fact is accepted");
    block_on(storage.flush_pending()).expect("the writes reach the medium");

    // The whitespace fact has a blob the rebuild will not index, and the other fact's
    // blob is gone, which the rebuild reports as no content rather than as an error.
    let hash = missing_blob.content_hash.to_string();
    use nex_fih::io::file_io::FileIo;
    block_on(storage.io.delete(&format!("blob/{hash}.bin"))).expect("the blob is removed");
    block_on(storage.io.delete(&format!("blob/{hash}.bin.meta"))).expect("the metadata is removed");
    recorder.0.lock().unwrap().clear();

    block_on(storage.rebuild_semantic()).expect("the index rebuilds");
    assert!(
        recorder.0.lock().unwrap().is_empty(),
        "a rebuild indexes only facts with text"
    );
}
