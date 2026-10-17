// The structural filter index and the record-map scan answer one question.
//
// `structural_fact_ids` prunes with `iter_prefix` over the leading axes and then
// re-applies the exact record-field predicates; `read_state_filtered` scans the record
// layer. The index exists for the pruning win, so a filter that prunes differently must
// still select the same records. The cases here are the day window, an open-ended bound,
// the origin and creator fingerprints, and a wanted id list, each compared against the
// baseline. The expected sets are asserted too, so two paths that agree on nothing do
// not pass as agreement.

#![cfg(feature = "structural-index")]

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use futures_executor::block_on;
use nex_fih::io::file_io::{FileIo, IoFuture};
use nex_fih::{AsyncFactCapable, AsyncFilterCapable, Content, Fact, FihStorage, Now, StateFilter};

const DAY_NS: u64 = 86_400_000_000_000;
/// The day the volume starts on, chosen inside the coordinate space rather than at 0.
const D0: u64 = 20_000;

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

/// A clock the test pins before each submit, so each record lands on a known day.
#[derive(Clone, Default)]
struct StepClock(Arc<AtomicU64>);

impl StepClock {
    fn set(&self, nanos: u64) {
        self.0.store(nanos, Ordering::Relaxed);
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

fn day(days: u64) -> u64 {
    days * DAY_NS
}

fn submit(
    clock: &StepClock,
    storage: &FihStorage<MemoryIo>,
    at_nanos: u64,
    origin: &str,
    creator: &str,
    payload: &str,
) -> String {
    clock.set(at_nanos);
    let fact = Fact::new(origin.into(), Content::from(payload), creator.into());
    block_on(storage.submit_fact(&fact)).expect("the fact is accepted");
    fact.id.to_string()
}

/// Five facts spanning three days, two origins, and two creators.
///
/// The layout is what makes the pruning cases distinguishable: the earliest two share
/// the origin, the next two share a creator, and the last one is alone on its day. The
/// medium is returned so a test can reopen the volume in a second session.
fn volume() -> (MemoryIo, FihStorage<MemoryIo>, [String; 5]) {
    let medium = MemoryIo::default();
    let clock = StepClock::default();
    let storage = FihStorage::with_clock(medium.clone(), "structural", Box::new(clock.clone()));
    let ids = [
        submit(
            &clock,
            &storage,
            day(D0) + 3_600_000_000_000,
            "site/a",
            "agent/one",
            "first",
        ),
        submit(
            &clock,
            &storage,
            day(D0) + 7_200_000_000_000,
            "site/a",
            "agent/two",
            "second",
        ),
        submit(
            &clock,
            &storage,
            day(D0 + 1),
            "site/b",
            "agent/one",
            "third",
        ),
        submit(
            &clock,
            &storage,
            day(D0 + 10),
            "site/b",
            "agent/one",
            "fourth",
        ),
        submit(
            &clock,
            &storage,
            day(D0 + 10) + 3_600_000_000_000,
            "site/a",
            "agent/one",
            "fifth",
        ),
    ];
    block_on(storage.flush_pending()).expect("the writes reach the medium");
    (medium, storage, ids)
}

fn sorted(mut ids: Vec<String>) -> Vec<String> {
    ids.sort_unstable();
    ids
}

/// The two paths over one filter, both sorted, and the assertion that they agree.
fn compare(storage: &FihStorage<MemoryIo>, filter: &StateFilter) -> Vec<String> {
    let mut indexed = storage.structural_fact_ids(filter);
    let mut scanned: Vec<String> = block_on(storage.read_state_filtered(filter))
        .facts
        .into_iter()
        .map(|fact| fact.id.to_string())
        .collect();
    indexed.sort_unstable();
    scanned.sort_unstable();
    assert_eq!(
        indexed, scanned,
        "the index and the scan disagree over {filter:?}"
    );
    indexed
}

#[test]
fn without_a_time_window_the_two_paths_select_the_whole_volume() {
    let (_, storage, ids) = volume();
    assert_eq!(
        compare(&storage, &StateFilter::default()),
        sorted(ids.to_vec())
    );
}

#[test]
fn a_bounded_day_window_selects_the_same_records_in_both_paths() {
    let (_, storage, ids) = volume();
    let filter = StateFilter {
        since: Some(day(D0 + 1).to_string()),
        until: Some(day(D0 + 10).to_string()),
        ..Default::default()
    };

    assert_eq!(
        compare(&storage, &filter),
        sorted(vec![ids[2].clone(), ids[3].clone()]),
        "the window includes its ends and nothing outside them"
    );
}

#[test]
fn a_window_that_covers_no_record_selects_nothing_in_both_paths() {
    let (_, storage, _) = volume();
    let filter = StateFilter {
        since: Some(day(D0 - 5).to_string()),
        until: Some(day(D0 - 1).to_string()),
        ..Default::default()
    };

    assert!(compare(&storage, &filter).is_empty());
}

#[test]
fn an_open_ended_bound_selects_the_same_records_in_both_paths() {
    let (_, storage, ids) = volume();

    let since_only = StateFilter {
        since: Some(day(D0 + 10).to_string()),
        ..Default::default()
    };
    assert_eq!(
        compare(&storage, &since_only),
        sorted(vec![ids[3].clone(), ids[4].clone()])
    );

    let until_only = StateFilter {
        until: Some(day(D0 + 1).to_string()),
        ..Default::default()
    };
    assert_eq!(
        compare(&storage, &until_only),
        sorted(vec![ids[0].clone(), ids[1].clone(), ids[2].clone()])
    );
}

#[test]
fn the_origin_and_creator_axes_select_the_same_records_in_both_paths() {
    let (_, storage, ids) = volume();

    let by_origin = StateFilter {
        origin: Some("site/a".into()),
        ..Default::default()
    };
    assert_eq!(
        compare(&storage, &by_origin),
        sorted(vec![ids[0].clone(), ids[1].clone(), ids[4].clone()])
    );

    let by_creator = StateFilter {
        creator: Some("agent/one".into()),
        ..Default::default()
    };
    assert_eq!(
        compare(&storage, &by_creator),
        sorted(vec![
            ids[0].clone(),
            ids[2].clone(),
            ids[3].clone(),
            ids[4].clone()
        ])
    );

    // Both fingerprints enter the prefix only when the origin is fixed first, and the
    // exact predicates still have to agree with them.
    let by_both = StateFilter {
        origin: Some("site/a".into()),
        creator: Some("agent/one".into()),
        ..Default::default()
    };
    assert_eq!(
        compare(&storage, &by_both),
        sorted(vec![ids[0].clone(), ids[4].clone()])
    );
}

#[test]
fn a_bounded_window_with_fingerprints_selects_the_same_records_in_both_paths() {
    // The origin and creator axes enter the prefix only when the window narrows it, and
    // their fingerprints are advisory: the exact predicates still have to agree.
    let (_, storage, ids) = volume();
    let filter = StateFilter {
        since: Some(day(D0 + 1).to_string()),
        until: Some(day(D0 + 10).to_string()),
        origin: Some("site/b".into()),
        creator: Some("agent/one".into()),
        ..Default::default()
    };
    assert_eq!(
        compare(&storage, &filter),
        sorted(vec![ids[2].clone(), ids[3].clone()])
    );

    let filter = StateFilter {
        since: Some(day(D0 + 1).to_string()),
        until: Some((day(D0 + 10) + 3_600_000_000_000).to_string()),
        origin: Some("site/a".into()),
        ..Default::default()
    };
    assert_eq!(compare(&storage, &filter), vec![ids[4].clone()]);
}

#[test]
fn a_wanted_id_list_selects_only_those_records_in_both_paths() {
    let (_, storage, ids) = volume();
    let filter = StateFilter {
        fact_ids: Some(vec![ids[1].clone(), ids[3].clone()]),
        ..Default::default()
    };
    assert_eq!(
        compare(&storage, &filter),
        sorted(vec![ids[1].clone(), ids[3].clone()])
    );

    // A wanted id that names nothing drops out rather than narrowing or widening the set.
    let filter = StateFilter {
        fact_ids: Some(vec![ids[1].clone(), "f_not_written".into()]),
        since: Some(day(D0).to_string()),
        until: Some(day(D0 + 10).to_string()),
        ..Default::default()
    };
    assert_eq!(compare(&storage, &filter), vec![ids[1].clone()]);
}

#[test]
fn the_two_paths_agree_after_a_reopen() {
    // The index a session did not build is rebuilt by `rebuild_cache`, so the agreement
    // holds for a volume rather than for one session's memory.
    let (medium, writer, ids) = volume();
    drop(writer);

    let reader = FihStorage::new(medium, "structural");
    block_on(reader.rebuild_cache()).expect("the volume reopens");

    let filter = StateFilter {
        origin: Some("site/a".into()),
        ..Default::default()
    };
    assert_eq!(
        compare(&reader, &filter),
        sorted(vec![ids[0].clone(), ids[1].clone(), ids[4].clone()])
    );
}
