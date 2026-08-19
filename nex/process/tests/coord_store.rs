// Unified 12-axis store synchronization (#170).
//
// The submit paths place records into the 12-axis coordinate store, and
// rebuild_cache repopulates it from io. Id enumeration and status reads
// therefore work after a reopen, which they did not before: the store
// was only populated by direct place_record callers like nex-calc, so
// all_*_ids returned empty after a reopen.

use futures_executor::block_on;
use nex_fih::{
    AsyncFactCapable, AsyncHintCapable, AsyncIntentCapable, AsyncScanCapable, AsyncStorageRead,
    AsyncTimeRangeCapable, Content, CoordId, Fact, FihStorage, Hint, Intent, IntentStatus,
};
use nexus_storage_sim::SimIo;

fn fact(id: &str, data: &[u8]) -> Fact {
    fact_with(id, data, "coord")
}

fn fact_with(id: &str, data: &[u8], origin: &str) -> Fact {
    Fact::with_id(
        CoordId::resolve(id),
        origin.into(),
        Content {
            mime_type: "text/plain".into(),
            data: data.to_vec(),
        },
        "t".into(),
    )
}

fn intent(id: &str, from_fact: &str) -> Intent {
    intent_with(id, from_fact, "t")
}

fn intent_with(id: &str, from_fact: &str, creator: &str) -> Intent {
    Intent {
        id: CoordId::resolve(id),
        from_facts: vec![CoordId::resolve(from_fact)],
        description: format!("intent {id}"),
        creator: creator.into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    }
}

fn hint(id: &str, content: &str) -> Hint {
    hint_with(id, content, "t")
}

fn hint_with(id: &str, content: &str, creator: &str) -> Hint {
    Hint {
        id: CoordId::resolve(id),
        content: content.into(),
        creator: creator.into(),
    }
}

#[test]
fn submit_paths_populate_id_enumeration_after_reopen() {
    block_on(async {
        let io = SimIo::new();
        {
            let store = FihStorage::new(io.clone(), "coord");
            store.submit_fact(&fact("f_a", b"a")).await.unwrap();
            store.submit_intent(&intent("i_a", "f_a")).await.unwrap();
            store.submit_hint(&hint("h_a", "note")).await.unwrap();
            store.flush_pending().await.unwrap();
            // In-session enumeration uses the hash-map fast path.
            assert_eq!(store.all_fact_ids().len(), 1);
            assert_eq!(store.all_intent_ids().len(), 1);
            assert_eq!(store.all_hint_ids().len(), 1);
        }
        {
            // Reopen: the hash maps are empty, so enumeration falls back
            // to the 12-axis store, which rebuild_cache must have filled.
            let store = FihStorage::new(io, "coord");
            store.rebuild_cache().await.unwrap();
            let fids = store.all_fact_ids();
            assert_eq!(fids.len(), 1, "fact ids must survive reopen via the store");
            assert!(
                fids.iter()
                    .any(|id| *id == CoordId::resolve("f_a").to_string())
            );
            assert_eq!(store.all_intent_ids().len(), 1);
            assert_eq!(store.all_hint_ids().len(), 1);
        }
    });
}

#[test]
fn intent_status_moves_are_visible_after_reopen() {
    block_on(async {
        let io = SimIo::new();
        {
            let store = FihStorage::new(io.clone(), "coord");
            store.submit_fact(&fact("f_base", b"base")).await.unwrap();
            store
                .submit_intent(&intent("i_mv", "f_base"))
                .await
                .unwrap();
            store.claim_intent("i_mv", "alice").await.unwrap();
            store.conclude_intent("i_mv", "done").await.unwrap();
            store.flush_pending().await.unwrap();
        }
        {
            let store = FihStorage::new(io, "coord");
            store.rebuild_cache().await.unwrap();
            let canonical = CoordId::resolve("i_mv").to_string();
            let (_, _, _, status, _) = store
                .get_intent_by_id(&canonical)
                .expect("intent must be readable from the store after reopen");
            assert!(
                matches!(status, IntentStatus::Concluded { .. }),
                "status move must vacate the old path and place the new one"
            );
            // The conclusion fact is placed in the store as well.
            assert_eq!(store.all_fact_ids().len(), 2, "base + conclusion");
        }
    });
}

/// Clock that advances one whole day plus one second per `now_nanos`
/// call, so facts land on distinct days (the path time is day-granular).
struct DayClock(std::sync::Mutex<u64>);

impl nex_core::Now for DayClock {
    fn now_nanos(&self) -> u64 {
        let mut now = self.0.lock().unwrap();
        let ts = *now;
        *now += 86_401_000_000_000;
        ts
    }

    fn now_secs(&self) -> u64 {
        *self.0.lock().unwrap() / 1_000_000_000
    }
}

#[test]
fn time_range_bounds_from_the_coordinate_store() {
    block_on(async {
        let io = SimIo::new();
        let store = FihStorage::with_clock(
            io,
            "coord",
            Box::new(DayClock(std::sync::Mutex::new(1_000_000_000_000_000_000))),
        );
        store.submit_fact(&fact("f_t1", b"a")).await.unwrap(); // t0
        store.submit_fact(&fact("f_t2", b"b")).await.unwrap(); // t0 + 1 day + 1s

        // Distinct days order the first and last Fact entries; the exact
        // timestamps come from the records.
        let range = store.time_range().await.expect("time range");
        assert_eq!(range.start, "1000000000000000000");
        assert_eq!(range.end, "1000086401000000000");
    });
}

/// Clock that advances one hour per `now_nanos` call, so facts share a
/// day but carry distinct timestamps.
struct HourClock(std::sync::Mutex<u64>);

impl nex_core::Now for HourClock {
    fn now_nanos(&self) -> u64 {
        let mut now = self.0.lock().unwrap();
        let ts = *now;
        *now += 3_600_000_000_000;
        ts
    }

    fn now_secs(&self) -> u64 {
        *self.0.lock().unwrap() / 1_000_000_000
    }
}

#[test]
fn time_range_is_exact_within_a_day() {
    block_on(async {
        let store = FihStorage::with_clock(
            SimIo::new(),
            "coord",
            Box::new(HourClock(std::sync::Mutex::new(1_000_000_000_000))),
        );
        store.submit_fact(&fact("f_a", b"a")).await.unwrap(); // t = 1e12
        store.submit_fact(&fact("f_b", b"b")).await.unwrap(); // t = 4.6e12

        let range = store.time_range().await.expect("time range");
        // Both facts share day 0; the exact bounds are the record
        // timestamps, not whichever fact the tree orders first within
        // the day.
        assert_eq!(range.start, "1000000000000");
        assert_eq!(range.end, "4600000000000");
    });
}

#[test]
fn conclusion_fact_carries_the_conclude_time() {
    block_on(async {
        let store = FihStorage::new(SimIo::new(), "coord");
        store.submit_fact(&fact("f_base", b"base")).await.unwrap();
        store
            .submit_intent(&intent("i_ts", "f_base"))
            .await
            .unwrap();
        store.claim_intent("i_ts", "alice").await.unwrap();
        store.conclude_intent("i_ts", "done").await.unwrap();

        // The conclusion fact is written with the conclude time, so the
        // range start is a real timestamp, not epoch.
        let range = store.time_range().await.expect("time range");
        assert!(
            range.start.parse::<u64>().unwrap() > 0,
            "conclusion fact must not sit at day zero"
        );

        let state = store.read_state().await;
        assert!(
            state
                .facts
                .iter()
                .any(|f| f.origin.starts_with("conclusion:i_ts")),
            "conclusion fact present with its origin"
        );
    });
}

#[test]
fn conclusion_fact_content_survives_reopen() {
    block_on(async {
        let io = SimIo::new();
        let intent_id = "i_c";
        {
            let store = FihStorage::new(io.clone(), "coord");
            store.submit_fact(&fact("f_base", b"base")).await.unwrap();
            store
                .submit_intent(&intent(intent_id, "f_base"))
                .await
                .unwrap();
            store.claim_intent(intent_id, "alice").await.unwrap();
            store.conclude_intent(intent_id, "resolved").await.unwrap();
        }
        {
            // The conclusion fact is blob-backed, so a reopen materializes
            // its content and a consistent hash from io.
            let store = FihStorage::new(io, "coord");
            store.rebuild_cache().await.unwrap();
            // Derive the conclusion id from the intent id so the test
            // survives the id-derivation scheme changing.
            let canonical = CoordId::resolve(&format!("f_concl_{intent_id}")).to_string();
            let (content, hash, origin, _creator) = store
                .get_fact_by_id(&canonical)
                .expect("conclusion fact readable after reopen");
            assert_eq!(String::from_utf8_lossy(&content.data), "resolved");
            assert_eq!(origin, format!("conclusion:{intent_id}"));
            assert!(
                hash.0.iter().any(|b| *b != 0),
                "content hash must be derived from the persisted blob"
            );
        }
    });
}

#[test]
fn scan_partition_matches_partition_strings() {
    block_on(async {
        let store = FihStorage::new(SimIo::new(), "coord");
        store
            .submit_fact(&fact_with("f_p1", b"a", "partition:alpha"))
            .await
            .unwrap();
        store
            .submit_fact(&fact_with("f_p2", b"b", "partition:beta"))
            .await
            .unwrap();
        store
            .submit_intent(&intent_with("i_a", "f_p1", "partition:alpha"))
            .await
            .unwrap();
        store
            .submit_hint(&hint_with("h_a", "note", "partition:alpha"))
            .await
            .unwrap();

        // Sanity: the submit paths did place the records.
        assert_eq!(store.all_fact_ids().len(), 2);

        // Facts match the origin field, intents and hints the creator
        // field (the existing per-type partition convention).
        let alpha = store.scan_partition("alpha").await.unwrap();
        assert_eq!(alpha.facts.len(), 1);
        assert_eq!(alpha.facts[0].id, CoordId::resolve("f_p1"));
        assert_eq!(alpha.intents.len(), 1);
        assert_eq!(alpha.hints.len(), 1);

        let beta = store.scan_partition("beta").await.unwrap();
        assert_eq!(beta.facts.len(), 1);
        assert_eq!(beta.facts[0].id, CoordId::resolve("f_p2"));
        assert!(beta.intents.is_empty());
        assert!(beta.hints.is_empty());
    });
}
