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
    text: Option<Vec<u8>>,
    features: Option<Vec<f32>>,
}

#[async_trait::async_trait]
impl SemanticStore for Recorder {
    async fn insert(&mut self, id: u32, load: &dyn RecordLoad) -> Result<(), String> {
        self.0.lock().unwrap().push(Call {
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
