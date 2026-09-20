// The by-id filters of a StateFilter.
//
// `fact_ids`, `intent_ids`, and `hint_ids` name records the caller wants, written either as a
// canonical id or as a label, and `CoordId::resolve` makes the two interchangeable. What these
// tests hold is that the wanted side is the only side that needs that derivation, that a
// repeated entry carries no meaning, and that a list which is absent and one which is empty are
// different answers.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_executor::block_on;
use nex_fih::io::file_io::{FileIo, IoFuture};
use nex_fih::{
    AsyncFactCapable, AsyncFilterCapable, AsyncHintCapable, AsyncIntentCapable, BoardState,
    Content, CoordId, Fact, FihStorage, Hint, Intent, StateFilter,
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

fn canonical(reference: &str) -> String {
    CoordId::resolve(reference).to_string()
}

fn fact_ids_of(state: &BoardState) -> Vec<String> {
    let mut ids: Vec<String> = state.facts.iter().map(|f| f.id.to_string()).collect();
    ids.sort_unstable();
    ids
}

fn intent_ids_of(state: &BoardState) -> Vec<String> {
    let mut ids: Vec<String> = state.intents.iter().map(|i| i.id.to_string()).collect();
    ids.sort_unstable();
    ids
}

fn hint_ids_of(state: &BoardState) -> Vec<String> {
    let mut ids: Vec<String> = state.hints.iter().map(|h| h.id.to_string()).collect();
    ids.sort_unstable();
    ids
}

/// A volume of three facts, two intents, and two hints, the first of each named by a label, so
/// that a wanted entry can be written both ways.
fn volume(medium: &MemoryIo) -> FihStorage<MemoryIo> {
    let storage = FihStorage::new(medium.clone(), "wanted");

    let named = Fact::with_id(
        CoordId::from_label("fact-one"),
        "origin/wanted".into(),
        Content::from("the first fact"),
        "writer".into(),
    );
    block_on(storage.submit_fact(&named)).expect("the fact is accepted");
    for payload in ["the second fact", "the third fact"] {
        let other = Fact::new(
            "origin/wanted".into(),
            Content::from(payload),
            "writer".into(),
        );
        block_on(storage.submit_fact(&other)).expect("the fact is accepted");
    }

    for (reference, description) in [
        ("intent-one", "the first intent"),
        ("intent-two", "the second intent"),
    ] {
        let intent = Intent::new(
            CoordId::resolve(reference),
            vec![named.id],
            None,
            description.to_string(),
            "writer".to_string(),
        );
        block_on(storage.submit_intent(&intent)).expect("the intent is accepted");
    }

    for (reference, content) in [
        ("hint-one", "the first hint"),
        ("hint-two", "the second hint"),
    ] {
        let hint = Hint {
            id: CoordId::resolve(reference),
            content: content.to_string(),
            creator: "writer".to_string(),
        };
        block_on(storage.submit_hint(&hint)).expect("the hint is accepted");
    }

    block_on(storage.flush_pending()).expect("the writes reach the medium");
    storage
}

/// Every kind has a wanted list of its own, and each selects the record it names.
#[test]
fn a_wanted_id_selects_the_record_it_names() {
    let medium = MemoryIo::default();
    let storage = volume(&medium);

    let state = block_on(storage.read_state_filtered(&StateFilter {
        fact_ids: Some(vec![canonical("fact-one")]),
        intent_ids: Some(vec![canonical("intent-two")]),
        hint_ids: Some(vec![canonical("hint-one")]),
        ..Default::default()
    }));

    assert_eq!(fact_ids_of(&state), vec![canonical("fact-one")]);
    assert_eq!(intent_ids_of(&state), vec![canonical("intent-two")]);
    assert_eq!(hint_ids_of(&state), vec![canonical("hint-one")]);
}

/// A wanted entry and the record it names are the same record whether the entry is written as a
/// label or as the canonical form the record layer stores.
#[test]
fn a_label_and_its_canonical_form_select_the_same_record() {
    let medium = MemoryIo::default();
    let storage = volume(&medium);

    let by_label = block_on(storage.read_state_filtered(&StateFilter {
        fact_ids: Some(vec!["fact-one".to_string()]),
        intent_ids: Some(vec!["intent-one".to_string()]),
        hint_ids: Some(vec!["hint-one".to_string()]),
        ..Default::default()
    }));
    let by_canonical = block_on(storage.read_state_filtered(&StateFilter {
        fact_ids: Some(vec![canonical("fact-one")]),
        intent_ids: Some(vec![canonical("intent-one")]),
        hint_ids: Some(vec![canonical("hint-one")]),
        ..Default::default()
    }));

    assert_eq!(fact_ids_of(&by_label), fact_ids_of(&by_canonical));
    assert_eq!(intent_ids_of(&by_label), intent_ids_of(&by_canonical));
    assert_eq!(hint_ids_of(&by_label), hint_ids_of(&by_canonical));
    assert_eq!(fact_ids_of(&by_label), vec![canonical("fact-one")]);
}

/// A list is a set of names, so writing one twice names the same record once.
#[test]
fn a_repeated_wanted_id_selects_the_record_once() {
    let medium = MemoryIo::default();
    let storage = volume(&medium);

    let state = block_on(storage.read_state_filtered(&StateFilter {
        fact_ids: Some(vec![canonical("fact-one"), canonical("fact-one")]),
        ..Default::default()
    }));

    assert_eq!(fact_ids_of(&state), vec![canonical("fact-one")]);
}

/// A wanted id that no record carries excludes every candidate, which is what a caller that
/// asks for records that are not there has to see.
#[test]
fn a_wanted_id_no_record_carries_excludes_every_candidate() {
    let medium = MemoryIo::default();
    let storage = volume(&medium);

    let state = block_on(storage.read_state_filtered(&StateFilter {
        fact_ids: Some(vec![canonical("a-fact-nobody-wrote")]),
        intent_ids: Some(vec![canonical("an-intent-nobody-wrote")]),
        hint_ids: Some(vec![canonical("a-hint-nobody-wrote")]),
        ..Default::default()
    }));

    assert!(state.facts.is_empty(), "a fact nobody wrote was selected");
    assert!(
        state.intents.is_empty(),
        "an intent nobody wrote was selected"
    );
    assert!(state.hints.is_empty(), "a hint nobody wrote was selected");
}

/// An absent list restricts nothing, while a present and empty one names no record. They are
/// different answers, and a reader that confused them would report a volume that is not there.
#[test]
fn an_absent_list_restricts_nothing_and_an_empty_one_names_nothing() {
    let medium = MemoryIo::default();
    let storage = volume(&medium);

    let absent = block_on(storage.read_state_filtered(&StateFilter::default()));
    let empty = block_on(storage.read_state_filtered(&StateFilter {
        fact_ids: Some(Vec::new()),
        intent_ids: Some(Vec::new()),
        hint_ids: Some(Vec::new()),
        ..Default::default()
    }));

    assert_eq!(3, absent.facts.len(), "an absent list restricted the facts");
    assert_eq!(
        2,
        absent.intents.len(),
        "an absent list restricted the intents"
    );
    assert_eq!(2, absent.hints.len(), "an absent list restricted the hints");

    assert!(empty.facts.is_empty(), "an empty list selected a fact");
    assert!(empty.intents.is_empty(), "an empty list selected an intent");
    assert!(empty.hints.is_empty(), "an empty list selected a hint");
}

/// The two filter paths answer one question one way, which is what lets a consumer move between
/// them. The wanted entry is a label here, because that is the side that needs normalizing.
#[cfg(feature = "structural-index")]
#[test]
fn the_two_filter_paths_agree_on_wanted_ids() {
    let medium = MemoryIo::default();
    let storage = volume(&medium);

    let filter = StateFilter {
        fact_ids: Some(vec![
            "fact-one".to_string(),
            canonical("a-fact-nobody-wrote"),
        ]),
        ..Default::default()
    };

    let scanned = fact_ids_of(&block_on(storage.read_state_filtered(&filter)));
    let mut walked = storage.structural_fact_ids(&filter);
    walked.sort_unstable();

    assert_eq!(walked, scanned, "the two paths select one set");
    assert_eq!(walked, vec![canonical("fact-one")]);
}
