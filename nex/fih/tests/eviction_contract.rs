// The eviction and lease contracts: what leaving the volume means for a record.
//
// Eviction removes a record from the maps and the medium, so a reopen does not bring it
// back. A hint is evicted by submission time, an intent only while it is unclaimed and
// old enough to be abandoned. Releasing a claimed intent returns it to the unclaimed
// state, and only the worker holding it may do that.

#![cfg(feature = "std")]

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use futures_executor::block_on;
use nex_fih::io::file_io::{FileIo, IoFuture};
use nex_fih::{
    AsyncEvictCapable, AsyncFactCapable, AsyncHintCapable, AsyncIntentCapable, AsyncStorageRead,
    BlackboardError, Content, CoordId, Fact, FihStorage, Hint, Intent, Now,
};

const S0: u64 = 1_700_000_000;

#[derive(Clone, Default)]
struct MemoryIo {
    map: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl MemoryIo {
    fn holds(&self, key: &str) -> bool {
        self.map.lock().unwrap().contains_key(key)
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

fn new_storage(medium: MemoryIo, clock: &StepClock) -> FihStorage<MemoryIo> {
    FihStorage::with_clock(medium, "evict", Box::new(clock.clone()))
}

fn base_fact(storage: &FihStorage<MemoryIo>) -> Fact {
    let fact = Fact::with_id(
        CoordId::resolve("f_base"),
        "origin/one".into(),
        Content::from("base"),
        "agent".into(),
    );
    block_on(storage.submit_fact(&fact)).expect("the base fact is accepted");
    fact
}

fn submit_intent(
    storage: &FihStorage<MemoryIo>,
    id: &str,
    from: &Fact,
    description: &str,
) -> Intent {
    let intent = Intent::new(
        CoordId::resolve(id),
        vec![from.id],
        None,
        description.into(),
        "agent".into(),
    );
    block_on(storage.submit_intent(&intent)).expect("the intent is accepted");
    intent
}

#[test]
fn evicting_stale_intents_keeps_the_claimed_one_and_the_medium_agrees() {
    let medium = MemoryIo::default();
    let clock = StepClock::default();
    let storage = new_storage(medium.clone(), &clock);
    let base = base_fact(&storage);

    clock.at_secs(S0);
    let old = submit_intent(&storage, "i_old", &base, "an abandoned intent");
    clock.at_secs(S0 + 1);
    let claimed = submit_intent(&storage, "i_claimed", &base, "an active intent");
    block_on(storage.claim_intent("i_claimed", "worker")).expect("the intent is claimed");

    clock.at_secs(S0 + 100);
    let evicted = block_on(storage.evict_stale_intents(60)).expect("the eviction runs");
    assert_eq!(evicted, 1, "only the unclaimed and old intent is evicted");

    let state = block_on(storage.read_state());
    assert_eq!(state.intents.len(), 1);
    assert_eq!(state.intents[0].id, claimed.id);
    assert_eq!(state.intents[0].worker.as_deref(), Some("worker"));

    block_on(storage.flush_pending()).expect("the delete reaches the medium");
    assert!(
        !medium.holds(&nex_fih::IntentRecord::intent_key(&old.id.to_string())),
        "the evicted intent's record is gone from the medium"
    );

    drop(storage);
    let reader = FihStorage::new(medium, "evict");
    block_on(reader.rebuild_cache()).expect("the volume reopens");
    let state = block_on(reader.read_state());
    assert_eq!(state.intents.len(), 1, "a reopen does not bring it back");
    assert_eq!(state.intents[0].id, claimed.id);
}

#[test]
fn evicting_hints_by_time_keeps_the_newer_ones_on_the_medium() {
    let medium = MemoryIo::default();
    let clock = StepClock::default();
    let storage = new_storage(medium.clone(), &clock);

    clock.at_secs(S0);
    let old = Hint {
        id: CoordId::resolve("h_old"),
        content: "an old hint".into(),
        creator: "agent".into(),
    };
    block_on(storage.submit_hint(&old)).expect("the hint is accepted");
    clock.at_secs(S0 + 100);
    let new = Hint {
        id: CoordId::resolve("h_new"),
        content: "a new hint".into(),
        creator: "agent".into(),
    };
    block_on(storage.submit_hint(&new)).expect("the hint is accepted");
    block_on(storage.flush_pending()).expect("the writes reach the medium");

    let evicted =
        block_on(storage.evict_before(&(S0 + 50).to_string())).expect("the eviction runs");
    assert_eq!(evicted, 1);

    block_on(storage.flush_pending()).expect("the delete reaches the medium");
    assert!(!medium.holds(&nex_fih::HintRecord::hint_key(&old.id.to_string())));
    assert!(medium.holds(&nex_fih::HintRecord::hint_key(&new.id.to_string())));

    drop(storage);
    let reader = FihStorage::new(medium, "evict");
    block_on(reader.rebuild_cache()).expect("the volume reopens");
    let state = block_on(reader.read_state());
    assert_eq!(state.hints.len(), 1);
    assert_eq!(state.hints[0].id, new.id);
}

#[test]
fn releasing_a_claimed_intent_returns_it_to_the_unclaimed_state() {
    let storage = new_storage(MemoryIo::default(), &StepClock::default());
    let base = base_fact(&storage);
    let intent = submit_intent(&storage, "i_release", &base, "an intent to release");

    block_on(storage.claim_intent("i_release", "worker/a")).expect("the intent is claimed");
    block_on(storage.release_intent("i_release", "worker/a")).expect("the holder releases it");

    let state = block_on(storage.read_state());
    assert_eq!(state.intents[0].id, intent.id);
    assert!(state.intents[0].worker.is_none(), "the claim is released");

    // A released intent is claimable again, by another worker.
    block_on(storage.claim_intent("i_release", "worker/b")).expect("the intent is claimable");
    let state = block_on(storage.read_state());
    assert_eq!(state.intents[0].worker.as_deref(), Some("worker/b"));
}

#[test]
fn only_the_holder_releases_a_claim_and_a_submitted_intent_needs_no_release() {
    let storage = new_storage(MemoryIo::default(), &StepClock::default());
    let base = base_fact(&storage);
    submit_intent(&storage, "i_lease", &base, "an intent to lease");

    // Releasing what nobody holds is the state the caller wanted, so it succeeds.
    block_on(storage.release_intent("i_lease", "worker/a")).expect("an unclaimed intent is free");

    block_on(storage.claim_intent("i_lease", "worker/a")).expect("the intent is claimed");
    let error = block_on(storage.release_intent("i_lease", "worker/b"))
        .expect_err("another worker cannot release the claim");
    assert!(matches!(error, BlackboardError::Forbidden(_)), "{error:?}");

    block_on(storage.conclude_intent("i_lease", "done")).expect("the holder concludes");
    let error = block_on(storage.release_intent("i_lease", "worker/a"))
        .expect_err("a concluded intent has no lease to release");
    assert!(matches!(error, BlackboardError::NotFound(_)), "{error:?}");
}
