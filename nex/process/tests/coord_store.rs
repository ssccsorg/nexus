// Unified 19-axis store synchronization (#170).
//
// The submit paths place records into the 19-axis coordinate store, and
// rebuild_cache repopulates it from io. Id enumeration and status reads
// therefore work after a reopen, which they did not before: the store
// was only populated by direct place_record callers like nex-calc, so
// all_*_ids returned empty after a reopen.

use futures_executor::block_on;
use nex_fih::{
    AsyncFactCapable, AsyncHintCapable, AsyncIntentCapable, AsyncScanCapable,
    AsyncTimeRangeCapable, Content, CoordId, Fact, FihStorage, Hint, Intent, IntentStatus,
};
use nexus_storage_sim::SimIo;

fn fact(id: &str, data: &[u8]) -> Fact {
    fact_with(id, data, "coord")
}

fn fact_with(id: &str, data: &[u8], origin: &str) -> Fact {
    Fact::with_id(
        CoordId::from_string(id),
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
        id: CoordId::from_string(id),
        from_facts: vec![CoordId::from_string(from_fact)],
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
        id: CoordId::from_string(id),
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
            // to the 19-axis store, which rebuild_cache must have filled.
            let store = FihStorage::new(io, "coord");
            store.rebuild_cache().await.unwrap();
            let fids = store.all_fact_ids();
            assert_eq!(fids.len(), 1, "fact ids must survive reopen via the store");
            assert!(
                fids.iter()
                    .any(|id| *id == CoordId::from_string("f_a").to_string())
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
            let canonical = CoordId::from_string("i_mv").to_string();
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

#[test]
fn scan_partition_uses_axis_predicates() {
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

        // Facts match the origin axis, intents and hints the creator
        // axis (the existing per-type partition convention).
        let alpha = store.scan_partition("alpha").await.unwrap();
        assert_eq!(alpha.facts.len(), 1);
        assert_eq!(alpha.facts[0].id, CoordId::from_string("f_p1"));
        assert_eq!(alpha.intents.len(), 1);
        assert_eq!(alpha.hints.len(), 1);

        let beta = store.scan_partition("beta").await.unwrap();
        assert_eq!(beta.facts.len(), 1);
        assert_eq!(beta.facts[0].id, CoordId::from_string("f_p2"));
        assert!(beta.intents.is_empty());
        assert!(beta.hints.is_empty());
    });
}
