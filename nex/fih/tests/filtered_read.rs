// The host's filtered read: every kind, every predicate, and every place content can
// come from.
//
// `read_state_filtered` is the read a host makes when it asks for less than the whole
// volume. The predicates differ per kind (facts match on origin, creator, and the
// submission time; intents on creator, creation time, and status; hints on creator),
// the wanted id lists choose which side to walk, content comes from the pending buffer
// or the medium, and paging applies to all three kinds at once. Each of those is a
// separate way to return the wrong set, so each is pinned here.

#![cfg(feature = "std")]

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use futures_executor::block_on;
use nex_fih::io::file_io::{FileIo, IoFuture};
use nex_fih::{
    AsyncEvictCapable, AsyncFactCapable, AsyncFilterCapable, AsyncHintCapable, AsyncIntentCapable,
    AsyncScanCapable, BoardState, Content, CoordId, Fact, FihStorage, Hint, Intent, IntentStatus,
    Now, PartitionData, StateFilter,
};

/// An in-memory medium shared by every storage that holds a clone of it.
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

#[derive(Clone, Default)]
struct StepClock(Arc<AtomicU64>);

impl StepClock {
    fn at_secs(&self, secs: u64) {
        self.0.store(secs * 1_000_000_000, Ordering::Relaxed);
    }
}

impl Now for StepClock {
    fn now_nanos(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }

    fn now_secs(&self) -> u64 {
        self.now_nanos() / 1_000_000_000
    }
}

const S0: u64 = 1_700_000_000;

fn fact(id: &str, origin: &str, creator: &str, payload: &str) -> Fact {
    Fact::with_id(
        CoordId::resolve(id),
        origin.into(),
        Content::from(payload),
        creator.into(),
    )
}

fn intent(id: &str, from: &str, creator: &str, description: &str) -> Intent {
    Intent::new(
        CoordId::resolve(id),
        vec![CoordId::resolve(from)],
        None,
        description.into(),
        creator.into(),
    )
}

fn hint(id: &str, creator: &str, content: &str) -> Hint {
    Hint {
        id: CoordId::resolve(id),
        content: content.into(),
        creator: creator.into(),
    }
}

fn submit_fact(storage: &FihStorage<MemoryIo>, fact: &Fact) {
    block_on(storage.submit_fact(fact)).expect("the fact is accepted");
}

fn submit_intent(storage: &FihStorage<MemoryIo>, intent: &Intent) {
    block_on(storage.submit_intent(intent)).expect("the intent is accepted");
}

fn submit_hint(storage: &FihStorage<MemoryIo>, hint: &Hint) {
    block_on(storage.submit_hint(hint)).expect("the hint is accepted");
}

/// Three facts, two of them sharing a payload, and two submitted intents.
///
/// Nothing is flushed, so every payload is in the pending buffer when the read runs.
fn pending_volume() -> FihStorage<MemoryIo> {
    let clock = StepClock::default();
    let storage = FihStorage::with_clock(MemoryIo::default(), "filtered", Box::new(clock.clone()));

    clock.at_secs(S0);
    submit_fact(
        &storage,
        &fact("f_one", "origin/a", "agent/one", "shared payload"),
    );
    clock.at_secs(S0 + 1);
    submit_fact(
        &storage,
        &fact("f_two", "origin/a", "agent/two", "shared payload"),
    );
    clock.at_secs(S0 + 10);
    submit_fact(
        &storage,
        &fact("f_three", "origin/b", "agent/one", "beta payload"),
    );

    clock.at_secs(S0 + 2);
    submit_intent(
        &storage,
        &intent("i_first", "f_one", "agent/one", "the first description"),
    );
    clock.at_secs(S0 + 3);
    submit_intent(&storage, &intent("i_second", "f_one", "agent/two", ""));

    submit_hint(&storage, &hint("h_one", "agent/one", "hint one"));
    submit_hint(&storage, &hint("h_two", "agent/two", "hint two"));

    storage
}

/// A volume whose intents cover the three lifecycle states, flushed and reopened.
fn lifecycle_volume() -> (MemoryIo, FihStorage<MemoryIo>) {
    let medium = MemoryIo::default();
    let clock = StepClock::default();
    let writer = FihStorage::with_clock(medium.clone(), "filtered", Box::new(clock.clone()));

    clock.at_secs(S0);
    submit_fact(
        &writer,
        &fact("f_one", "origin/a", "agent/one", "shared payload"),
    );
    clock.at_secs(S0 + 1);
    submit_fact(
        &writer,
        &fact("f_two", "origin/a", "agent/two", "shared payload"),
    );
    clock.at_secs(S0 + 10);
    submit_fact(
        &writer,
        &fact("f_three", "origin/b", "agent/one", "beta payload"),
    );

    clock.at_secs(S0 + 2);
    submit_intent(
        &writer,
        &intent("i_first", "f_one", "agent/one", "the first description"),
    );
    clock.at_secs(S0 + 3);
    submit_intent(&writer, &intent("i_second", "f_one", "agent/two", ""));
    clock.at_secs(S0 + 4);
    submit_intent(
        &writer,
        &intent("i_third", "f_one", "agent/one", "shared description"),
    );
    clock.at_secs(S0 + 5);
    submit_intent(
        &writer,
        &intent("i_fourth", "f_one", "agent/one", "shared description"),
    );

    // A claimed intent, and one claimed, heartbeaten, and concluded.
    clock.at_secs(S0 + 20);
    block_on(writer.claim_intent("i_third", "worker/one")).expect("the intent is claimed");
    clock.at_secs(S0 + 30);
    block_on(writer.heartbeat("i_third", "worker/one")).expect("the heartbeat lands");
    clock.at_secs(S0 + 40);
    block_on(writer.conclude_intent("i_third", "resolved")).expect("the intent concludes");

    clock.at_secs(S0 + 50);
    block_on(writer.claim_intent("i_fourth", "worker/two")).expect("the intent is claimed");

    submit_hint(&writer, &hint("h_one", "agent/one", "hint one"));
    submit_hint(&writer, &hint("h_two", "agent/two", "hint two"));
    block_on(writer.flush_pending()).expect("the writes reach the medium");
    drop(writer);

    let reader = FihStorage::new(medium.clone(), "filtered");
    block_on(reader.rebuild_cache()).expect("the volume reopens");
    (medium, reader)
}

/// The three record lists a state read and a partition scan both carry.
trait Records {
    fn facts(&self) -> &[Fact];
    fn intents(&self) -> &[Intent];
    fn hints(&self) -> &[Hint];
}

impl Records for BoardState {
    fn facts(&self) -> &[Fact] {
        &self.facts
    }

    fn intents(&self) -> &[Intent] {
        &self.intents
    }

    fn hints(&self) -> &[Hint] {
        &self.hints
    }
}

impl Records for PartitionData {
    fn facts(&self) -> &[Fact] {
        &self.facts
    }

    fn intents(&self) -> &[Intent] {
        &self.intents
    }

    fn hints(&self) -> &[Hint] {
        &self.hints
    }
}

fn fact_ids(state: &impl Records) -> Vec<String> {
    let mut ids: Vec<String> = state.facts().iter().map(|f| f.id.to_string()).collect();
    ids.sort_unstable();
    ids
}

fn intent_ids(state: &impl Records) -> Vec<String> {
    let mut ids: Vec<String> = state.intents().iter().map(|i| i.id.to_string()).collect();
    ids.sort_unstable();
    ids
}

fn hint_ids(state: &impl Records) -> Vec<String> {
    let mut ids: Vec<String> = state.hints().iter().map(|h| h.id.to_string()).collect();
    ids.sort_unstable();
    ids
}

fn ids(names: &[&str]) -> Vec<String> {
    let mut ids: Vec<String> = names
        .iter()
        .map(|name| CoordId::resolve(name).to_string())
        .collect();
    ids.sort_unstable();
    ids
}

/// The fact a concluded intent produces, which is a record like any other.
///
/// The conclusion is addressed by the intent's canonical id, so the helper resolves the
/// label the test concluded with before deriving the id.
fn conclusion_fact() -> String {
    CoordId::resolve(&format!("f_concl_{}", CoordId::resolve("i_third"))).to_string()
}

fn read(storage: &FihStorage<MemoryIo>, filter: StateFilter) -> BoardState {
    block_on(storage.read_state_filtered(&filter))
}

fn find_fact<'a>(state: &'a impl Records, id: &str) -> &'a Fact {
    let wanted = CoordId::resolve(id);
    state
        .facts()
        .iter()
        .find(|fact| fact.id == wanted)
        .unwrap_or_else(|| panic!("the read does not hold {id}"))
}

fn find_intent<'a>(state: &'a impl Records, id: &str) -> &'a Intent {
    let wanted = CoordId::resolve(id);
    state
        .intents()
        .iter()
        .find(|intent| intent.id == wanted)
        .unwrap_or_else(|| panic!("the read does not hold {id}"))
}

#[test]
fn a_pending_volume_answers_with_the_payloads_it_holds() {
    let storage = pending_volume();
    let state = read(&storage, StateFilter::default());

    assert_eq!(fact_ids(&state), ids(&["f_one", "f_three", "f_two"]));
    assert_eq!(intent_ids(&state), ids(&["i_first", "i_second"]));
    assert_eq!(hint_ids(&state), ids(&["h_one", "h_two"]));

    // Two facts share a payload, so the read has one blob to resolve for both.
    for (id, payload) in [
        ("f_one", "shared payload"),
        ("f_two", "shared payload"),
        ("f_three", "beta payload"),
    ] {
        let content = &find_fact(&state, id).content;
        assert_eq!(content.data, payload.as_bytes(), "{id}");
        assert_eq!(content.mime_type, "text/plain", "{id}");
    }

    assert_eq!(
        find_intent(&state, "i_first").description,
        "the first description"
    );
    assert_eq!(
        find_intent(&state, "i_second").description,
        CoordId::resolve("i_second").to_string(),
        "an intent with no description falls back to its identifier"
    );
    assert!(find_intent(&state, "i_first").worker.is_none());
    assert!(find_intent(&state, "i_first").to_fact_id.is_none());
    assert!(!find_intent(&state, "i_first").is_concluded);

    let hint = state
        .hints
        .iter()
        .find(|hint| hint.id == CoordId::resolve("h_one"))
        .expect("the read holds the hint");
    assert_eq!(hint.content, "hint one");
}

#[test]
fn each_kind_is_filtered_by_its_own_fields() {
    let (_, storage) = lifecycle_volume();

    // Origin is a fact predicate; intents and hints are unaffected by it.
    let state = read(
        &storage,
        StateFilter {
            origin: Some("origin/a".into()),
            ..Default::default()
        },
    );
    assert_eq!(fact_ids(&state), ids(&["f_one", "f_two"]));
    assert_eq!(intent_ids(&state).len(), 4);
    assert_eq!(hint_ids(&state).len(), 2);

    // Creator applies to all three kinds, with the per-type meaning.
    let state = read(
        &storage,
        StateFilter {
            creator: Some("agent/one".into()),
            ..Default::default()
        },
    );
    assert_eq!(fact_ids(&state), ids(&["f_one", "f_three"]));
    assert_eq!(
        intent_ids(&state),
        ids(&["i_first", "i_fourth", "i_third"]),
        "the conclusion fact does not add an intent"
    );
    assert_eq!(hint_ids(&state), ids(&["h_one"]));

    // Status is an intent predicate only.
    for (status, expected) in [
        ("submitted", ids(&["i_first", "i_second"])),
        ("claimed", ids(&["i_fourth"])),
        ("concluded", ids(&["i_third"])),
    ] {
        let state = read(
            &storage,
            StateFilter {
                status: Some(status.into()),
                ..Default::default()
            },
        );
        assert_eq!(intent_ids(&state), expected, "status {status}");
        assert_eq!(fact_ids(&state).len(), 4, "status does not filter facts");
    }
}

#[test]
fn time_bounds_use_each_kinds_own_unit() {
    let (_, storage) = lifecycle_volume();

    // Facts carry nanoseconds since the epoch, and so do the intent bounds once the
    // record's whole seconds are converted.
    let state = read(
        &storage,
        StateFilter {
            since: Some(((S0 + 5) * 1_000_000_000).to_string()),
            ..Default::default()
        },
    );
    assert_eq!(fact_ids(&state), ids(&["f_three", &conclusion_fact()]));

    let state = read(
        &storage,
        StateFilter {
            until: Some(((S0 + 1) * 1_000_000_000).to_string()),
            ..Default::default()
        },
    );
    assert_eq!(fact_ids(&state), ids(&["f_one", "f_two"]));

    let state = read(
        &storage,
        StateFilter {
            since: Some(((S0 + 4) * 1_000_000_000).to_string()),
            ..Default::default()
        },
    );
    assert_eq!(intent_ids(&state), ids(&["i_fourth", "i_third"]));

    let state = read(
        &storage,
        StateFilter {
            until: Some(((S0 + 2) * 1_000_000_000).to_string()),
            ..Default::default()
        },
    );
    assert_eq!(intent_ids(&state), ids(&["i_first"]));

    // A bound that does not parse is no bound rather than an empty result.
    let state = read(
        &storage,
        StateFilter {
            since: Some("yesterday".into()),
            until: Some("".into()),
            ..Default::default()
        },
    );
    assert_eq!(
        fact_ids(&state),
        ids(&["f_one", "f_three", "f_two", &conclusion_fact()])
    );
}

#[test]
fn wanted_ids_choose_the_side_to_walk_and_an_empty_list_names_nothing() {
    let (_, storage) = lifecycle_volume();

    let state = read(
        &storage,
        StateFilter {
            fact_ids: Some(vec!["f_two".into()]),
            ..Default::default()
        },
    );
    assert_eq!(fact_ids(&state), ids(&["f_two"]));
    assert_eq!(
        intent_ids(&state).len(),
        4,
        "the other kinds are unrestricted"
    );

    let state = read(
        &storage,
        StateFilter {
            intent_ids: Some(vec!["i_second".into()]),
            ..Default::default()
        },
    );
    assert_eq!(intent_ids(&state), ids(&["i_second"]));

    let state = read(
        &storage,
        StateFilter {
            hint_ids: Some(vec!["h_two".into()]),
            ..Default::default()
        },
    );
    assert_eq!(hint_ids(&state), ids(&["h_two"]));

    // A present and empty list is a restriction that names no record.
    let state = read(
        &storage,
        StateFilter {
            fact_ids: Some(Vec::new()),
            intent_ids: Some(Vec::new()),
            hint_ids: Some(Vec::new()),
            ..Default::default()
        },
    );
    assert!(state.facts.is_empty());
    assert!(state.intents.is_empty());
    assert!(state.hints.is_empty());

    // A wanted id that names nothing drops out rather than failing the read.
    let state = read(
        &storage,
        StateFilter {
            fact_ids: Some(vec!["f_two".into(), "f_absent".into()]),
            ..Default::default()
        },
    );
    assert_eq!(fact_ids(&state), ids(&["f_two"]));
}

#[test]
fn paging_applies_to_every_kind_at_once() {
    let (_, storage) = lifecycle_volume();

    let state = read(
        &storage,
        StateFilter {
            limit: Some(1),
            ..Default::default()
        },
    );
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.intents.len(), 1);
    assert_eq!(state.hints.len(), 1);

    let state = read(
        &storage,
        StateFilter {
            offset: Some(1),
            ..Default::default()
        },
    );
    assert_eq!(state.facts.len(), 3);
    assert_eq!(state.intents.len(), 3);
    assert_eq!(state.hints.len(), 1);

    let state = read(
        &storage,
        StateFilter {
            offset: Some(10),
            ..Default::default()
        },
    );
    assert!(state.facts.is_empty());
    assert!(state.intents.is_empty());
    assert!(state.hints.is_empty());

    let state = read(
        &storage,
        StateFilter {
            limit: Some(0),
            ..Default::default()
        },
    );
    assert!(state.facts.is_empty());
    assert!(state.intents.is_empty());
    assert!(state.hints.is_empty());
}

#[test]
fn the_model_reports_seconds_while_the_record_reports_nanoseconds() {
    // The model and the JSON wire form report seconds. The by-name record APIs keep the
    // storage unit, which is nanoseconds, so the boundary is explicit on both sides.
    let (_, storage) = lifecycle_volume();
    let canonical = CoordId::resolve("i_third").to_string();

    let state = read(&storage, StateFilter::default());
    let concluded = find_intent(&state, "i_third");
    assert_eq!(concluded.created_at, Some(S0 + 4));
    assert_eq!(concluded.concluded_at, Some(S0 + 40));
    let wire = serde_json::to_value(concluded).expect("the intent serializes");
    assert_eq!(wire["created_at"].as_u64(), Some(S0 + 4));
    assert_eq!(wire["concluded_at"].as_u64(), Some(S0 + 40));

    let (record, _) = block_on(storage.get_intent("i_third")).expect("the record");
    assert_eq!(record.created_at, (S0 + 4) * 1_000_000_000);
    match record.status {
        IntentStatus::Concluded { concluded_at, .. } => {
            assert_eq!(concluded_at, (S0 + 40) * 1_000_000_000)
        }
        other => panic!("the intent is not concluded: {other:?}"),
    }

    let (_, _, _, status, created_at) = storage
        .get_intent_by_id(&canonical)
        .expect("the record map holds the status");
    assert_eq!(created_at, (S0 + 4) * 1_000_000_000);
    match status {
        IntentStatus::Concluded { concluded_at, .. } => {
            assert_eq!(concluded_at, (S0 + 40) * 1_000_000_000)
        }
        other => panic!("the intent is not concluded: {other:?}"),
    }
}

#[test]
fn a_reopened_volume_materializes_content_from_the_medium() {
    let (_, storage) = lifecycle_volume();
    let state = read(&storage, StateFilter::default());

    assert_eq!(
        fact_ids(&state),
        ids(&["f_one", "f_three", "f_two", &conclusion_fact()])
    );

    // The two intents with one description share a blob, so the medium is asked once.
    for id in ["i_third", "i_fourth"] {
        assert_eq!(
            find_intent(&state, id).description,
            "shared description",
            "{id}"
        );
    }
    assert_eq!(find_fact(&state, "f_three").content.data, b"beta payload");

    let concluded = find_intent(&state, "i_third");
    assert_eq!(concluded.worker.as_deref(), Some("worker/one"));
    // The concluded state keeps the worker and the conclusion time; the last heartbeat
    // belongs to the claimed state it left.
    assert_eq!(concluded.last_heartbeat_at, None);
    assert_eq!(
        concluded.to_fact_id,
        Some(CoordId::resolve(&conclusion_fact()))
    );
    assert!(concluded.is_concluded);
    // The public model reports seconds, the same unit as `created_at`.
    assert_eq!(concluded.created_at, Some(S0 + 4));
    assert_eq!(concluded.concluded_at, Some(S0 + 40));

    let claimed = find_intent(&state, "i_fourth");
    assert_eq!(claimed.worker.as_deref(), Some("worker/two"));
    assert_eq!(claimed.last_heartbeat_at, Some(S0 + 50));
    assert!(claimed.to_fact_id.is_none());
    assert!(!claimed.is_concluded);
    assert!(claimed.concluded_at.is_none());
}

#[test]
fn a_pending_delete_is_skipped_rather_than_read_as_content() {
    let clock = StepClock::default();
    let storage = FihStorage::with_clock(MemoryIo::default(), "filtered", Box::new(clock.clone()));
    clock.at_secs(S0);
    submit_fact(&storage, &fact("f_kept", "origin/a", "agent/one", "kept"));
    submit_hint(&storage, &hint("h_gone", "agent/one", "gone"));
    block_on(storage.flush_pending()).expect("the writes reach the medium");

    // Eviction enqueues the hint's delete, so the next read meets a pending op that is
    // not a write. It must be skipped rather than treated as a payload.
    let evicted = block_on(storage.evict_before(&u64::MAX.to_string())).expect("the hint evicts");
    assert_eq!(evicted, 1);

    let state = read(&storage, StateFilter::default());
    assert_eq!(fact_ids(&state), ids(&["f_kept"]));
    assert!(
        state.hints.is_empty(),
        "the evicted hint is not in the read"
    );
}

/// A partition volume: alpha holds every kind, beta holds one fact.
fn partition_volume() -> (MemoryIo, FihStorage<MemoryIo>) {
    let medium = MemoryIo::default();
    let clock = StepClock::default();
    let writer = FihStorage::with_clock(medium.clone(), "partition", Box::new(clock.clone()));

    clock.at_secs(S0);
    submit_fact(
        &writer,
        &fact(
            "f_alpha",
            "partition:alpha",
            "partition:alpha",
            "alpha payload",
        ),
    );
    submit_fact(
        &writer,
        &fact("f_beta", "partition:beta", "partition:beta", "beta payload"),
    );
    clock.at_secs(S0 + 1);
    submit_intent(
        &writer,
        &intent("i_alpha", "f_alpha", "partition:alpha", "alpha description"),
    );
    submit_intent(&writer, &intent("i_bare", "f_alpha", "partition:alpha", ""));
    submit_hint(&writer, &hint("h_alpha", "partition:alpha", "alpha hint"));
    submit_hint(&writer, &hint("h_beta", "partition:beta", "beta hint"));

    clock.at_secs(S0 + 2);
    block_on(writer.claim_intent("i_alpha", "worker/alpha")).expect("the intent is claimed");

    // A concluded intent, so the partition scan has to map that state too.
    submit_intent(
        &writer,
        &intent(
            "i_gamma",
            "f_alpha",
            "partition:alpha",
            "concluded description",
        ),
    );
    clock.at_secs(S0 + 3);
    block_on(writer.claim_intent("i_gamma", "worker/gamma")).expect("the intent is claimed");
    clock.at_secs(S0 + 4);
    block_on(writer.conclude_intent("i_gamma", "done")).expect("the intent concludes");

    block_on(writer.flush_pending()).expect("the writes reach the medium");
    drop(writer);

    let reader = FihStorage::new(medium.clone(), "partition");
    block_on(reader.rebuild_cache()).expect("the volume reopens");
    (medium, reader)
}

#[test]
fn a_partition_scan_returns_the_records_that_name_it() {
    let (_, storage) = partition_volume();

    let alpha = block_on(storage.scan_partition("alpha")).expect("the scan succeeds");
    assert_eq!(alpha.partition, "alpha");
    assert_eq!(fact_ids(&alpha), ids(&["f_alpha"]));
    assert_eq!(intent_ids(&alpha), ids(&["i_alpha", "i_bare", "i_gamma"]));
    assert_eq!(hint_ids(&alpha), ids(&["h_alpha"]));

    assert_eq!(find_fact(&alpha, "f_alpha").content.data, b"alpha payload");
    assert_eq!(
        find_intent(&alpha, "i_alpha").description,
        "alpha description"
    );
    assert_eq!(
        find_intent(&alpha, "i_bare").description,
        CoordId::resolve("i_bare").to_string()
    );
    let claimed = find_intent(&alpha, "i_alpha");
    assert_eq!(claimed.worker.as_deref(), Some("worker/alpha"));
    assert_eq!(claimed.last_heartbeat_at, Some(S0 + 2));

    let concluded = find_intent(&alpha, "i_gamma");
    assert_eq!(concluded.description, "concluded description");
    assert_eq!(concluded.worker.as_deref(), Some("worker/gamma"));
    assert_eq!(
        concluded.to_fact_id,
        Some(CoordId::resolve(&format!(
            "f_concl_{}",
            CoordId::resolve("i_gamma")
        )))
    );
    assert!(concluded.is_concluded);
    assert_eq!(concluded.created_at, Some(S0 + 2));
    assert_eq!(concluded.concluded_at, Some(S0 + 4));

    let beta = block_on(storage.scan_partition("beta")).expect("the scan succeeds");
    assert_eq!(fact_ids(&beta), ids(&["f_beta"]));
    assert!(beta.intents.is_empty());
    assert_eq!(hint_ids(&beta), ids(&["h_beta"]));

    let absent = block_on(storage.scan_partition("absent")).expect("the scan succeeds");
    assert!(absent.facts.is_empty());
    assert!(absent.intents.is_empty());
    assert!(absent.hints.is_empty());
}
