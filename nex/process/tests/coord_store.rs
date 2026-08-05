// Unified 19-axis store synchronization (#170).
//
// The submit paths place records into the 19-axis coordinate store, and
// rebuild_cache repopulates it from io. Id enumeration and status reads
// therefore work after a reopen, which they did not before: the store
// was only populated by direct place_record callers like nex-calc, so
// all_*_ids returned empty after a reopen.

use futures_executor::block_on;
use nex_fih::{
    AsyncFactCapable, AsyncHintCapable, AsyncIntentCapable, Content, CoordId, Fact, FihStorage,
    Hint, Intent, IntentStatus,
};
use nexus_storage_sim::SimIo;

fn fact(id: &str, data: &[u8]) -> Fact {
    Fact::with_id(
        CoordId::from_string(id),
        "coord".into(),
        Content {
            mime_type: "text/plain".into(),
            data: data.to_vec(),
        },
        "t".into(),
    )
}

fn intent(id: &str, from_fact: &str) -> Intent {
    Intent {
        id: CoordId::from_string(id),
        from_facts: vec![CoordId::from_string(from_fact)],
        description: format!("intent {id}"),
        creator: "t".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    }
}

fn hint(id: &str, content: &str) -> Hint {
    Hint {
        id: CoordId::from_string(id),
        content: content.into(),
        creator: "t".into(),
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
