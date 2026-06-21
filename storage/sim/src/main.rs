// ── storage/sim verification runner ──────────────────────────────────────
//
// Usage: cargo run -p nexus-storage-sim
//
// Exercises every storage capability end-to-end and exits with 0 only when
// every verification step passes. Designed to be run as a smoke test in CI
// or during development (cargo run, not cargo test).

use nexus_model::{
    AsyncEvictCapable, AsyncFactCapable, AsyncFilterCapable, AsyncFlushCapable, AsyncHintCapable,
    AsyncIntentCapable, AsyncStorageRead, Fact, FihHash, FlushCursor, Hint, Intent, StateFilter,
};
use nexus_storage_sim::{intent_status, FihStorage, SimIo, SyncFileIo};

#[tokio::main]
async fn main() {
    eprintln!("+-----------------------------------------------------------+");
    eprintln!("| nexus-storage-sim verification runner                      |");
    eprintln!("| Phase 3: AsyncFileIo + FlushCapable + FsIo              |");
    eprintln!("+-----------------------------------------------------------+");
    eprintln!();

    let mut total = 0u64;
    let mut passed = 0u64;

    macro_rules! check {
        ($label:expr, $body:block) => {{
            total += 1;
            eprint!("  [{total:>2}] {:<44} ", $label);
            #[allow(unused_must_use)]
            let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| $body));
            match ok {
                Ok(()) => {
                    eprintln!("PASS");
                    passed += 1;
                }
                Err(panic) => {
                    let msg = if let Some(s) = panic.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = panic.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "unknown panic".to_string()
                    };
                    eprintln!("FAIL -- panicked: {msg}");
                }
            }
        }};
    }

    macro_rules! check_async {
        ($label:expr, $body:block) => {{
            total += 1;
            eprint!("  [{total:>2}] {:<44} ", $label);
            #[allow(unused_must_use)]
            let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                futures_executor::block_on(async { $body })
            }));
            match ok {
                Ok(()) => {
                    eprintln!("PASS");
                    passed += 1;
                }
                Err(panic) => {
                    let msg = if let Some(s) = panic.downcast_ref::<&str>() {
                        s.to_string()
                    } else if let Some(s) = panic.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "unknown panic".to_string()
                    };
                    eprintln!("FAIL -- panicked: {msg}");
                }
            }
        }};
    }

    // ── 1. Basic FIH lifecycle ────────────────────────────────────────

    check_async!("submit_fact + read_state", {
        let store = FihStorage::new(SimIo::new(), "verify");
        AsyncFactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash::from_hex("f001"),
                origin: "verify".into(),
                content: "hello world".into(),
                creator: "v".into(),
            },
        )
        .await
        .unwrap();
        let state = AsyncStorageRead::read_state(&store).await;
        assert_eq!(state.facts.len(), 1, "expected 1 fact");
        assert_eq!(state.facts[0].id.to_string(), "f001");
    });

    check_async!("submit_intent requires existing fact", {
        let store = FihStorage::new(SimIo::new(), "verify");
        let result = AsyncIntentCapable::submit_intent(
            &store,
            &Intent {
                id: FihHash::from_hex("i001"),
                from_facts: vec![FihHash::from_hex("f_nonexistent")],
                description: "test".into(),
                creator: "v".into(),
                worker: None,
                to_fact_id: None,
                last_heartbeat_at: None,
                created_at: None,
                is_concluded: false,
                concluded_at: None,
            },
        )
        .await;
        assert!(
            result.is_err(),
            "must reject intent referencing missing fact"
        );
    });

    check_async!("full intent lifecycle", {
        let store = FihStorage::new(SimIo::new(), "verify");
        AsyncFactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash::from_hex("f_base"),
                origin: "verify".into(),
                content: "base data".into(),
                creator: "v".into(),
            },
        )
        .await
        .unwrap();
        AsyncIntentCapable::submit_intent(
            &store,
            &Intent {
                id: FihHash::from_hex("i001"),
                from_facts: vec![FihHash::from_hex("f_base")],
                description: "analyze base".into(),
                creator: "v".into(),
                worker: None,
                to_fact_id: None,
                last_heartbeat_at: None,
                created_at: None,
                is_concluded: false,
                concluded_at: None,
            },
        )
        .await
        .unwrap();
        AsyncIntentCapable::claim_intent(&store, "i001", "alice")
            .await
            .unwrap();
        AsyncIntentCapable::heartbeat(&store, "i001", "alice")
            .await
            .unwrap();
        let concl = AsyncIntentCapable::conclude_intent(&store, "i001", "result data")
            .await
            .unwrap();
        assert!(concl.id.to_string().starts_with("f_concl_"));
        let state = AsyncStorageRead::read_state(&store).await;
        assert_eq!(state.facts.len(), 2, "base + conclusion");
    });

    check_async!("double claim rejected", {
        let store = FihStorage::new(SimIo::new(), "verify");
        AsyncFactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash::from_hex("f_base"),
                origin: "v".into(),
                content: "x".into(),
                creator: "v".into(),
            },
        )
        .await
        .unwrap();
        AsyncIntentCapable::submit_intent(
            &store,
            &Intent {
                id: FihHash::from_hex("i001"),
                from_facts: vec![FihHash::from_hex("f_base")],
                description: "test".into(),
                creator: "v".into(),
                worker: None,
                to_fact_id: None,
                last_heartbeat_at: None,
                created_at: None,
                is_concluded: false,
                concluded_at: None,
            },
        )
        .await
        .unwrap();
        AsyncIntentCapable::claim_intent(&store, "i001", "alice")
            .await
            .unwrap();
        let second = AsyncIntentCapable::claim_intent(&store, "i001", "bob").await;
        assert!(second.is_err(), "double claim must be rejected");
    });

    // ── 2. Hint operations ────────────────────────────────────────────

    check_async!("submit_hint + read_state", {
        let store = FihStorage::new(SimIo::new(), "verify");
        AsyncHintCapable::submit_hint(
            &store,
            &Hint {
                id: FihHash::from_hex("h001"),
                content: "ephemeral note".into(),
                creator: "v".into(),
            },
        )
        .await
        .unwrap();
        let state = AsyncStorageRead::read_state(&store).await;
        assert_eq!(state.hints.len(), 1);
    });

    // ── 3. Flush + rebuild ────────────────────────────────────────────

    check_async!("flush + rebuild preserves data", {
        let io = SimIo::new();
        let store = FihStorage::new(io.clone(), "verify");
        AsyncFactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash::from_hex("f001"),
                origin: "v".into(),
                content: "flush test".into(),
                creator: "v".into(),
            },
        )
        .await
        .unwrap();
        futures_executor::block_on(store.flush_pending()).unwrap();
        let store2 = FihStorage::new(io, "verify");
        futures_executor::block_on(store2.rebuild_cache()).unwrap();
        let state = AsyncStorageRead::read_state(&store2).await;
        assert_eq!(state.facts.len(), 1);
        assert_eq!(state.facts[0].content.data, b"flush test");
    });

    check_async!("flush_cursor advances", {
        let store = FihStorage::new(SimIo::new(), "verify");
        AsyncFactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash::from_hex("f001"),
                origin: "v".into(),
                content: "a".into(),
                creator: "v".into(),
            },
        )
        .await
        .unwrap();
        let cursor = FlushCursor {
            last_flushed_at: 0,
            partition: "default".into(),
        };
        let result = AsyncFlushCapable::flush_since(&store, &cursor)
            .await
            .unwrap();
        assert!(result.records_flushed > 0);
        assert!(result.new_cursor.last_flushed_at > 0);
    });

    check_async!("flush_empty_delta", {
        let store = FihStorage::new(SimIo::new(), "verify");
        let cursor = FlushCursor {
            last_flushed_at: u64::MAX,
            partition: "default".into(),
        };
        let result = AsyncFlushCapable::flush_since(&store, &cursor)
            .await
            .unwrap();
        assert_eq!(result.records_flushed, 0);
    });

    check_async!("flush writes to io", {
        let io = SimIo::new();
        let store = FihStorage::new(io.clone(), "verify");
        AsyncFactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash::from_hex("f001"),
                origin: "v".into(),
                content: "data".into(),
                creator: "v".into(),
            },
        )
        .await
        .unwrap();
        let cursor = FlushCursor {
            last_flushed_at: 0,
            partition: "default".into(),
        };
        AsyncFlushCapable::flush_since(&store, &cursor)
            .await
            .unwrap();
        let keys = SyncFileIo::new(io).list("flush/").unwrap();
        assert!(!keys.is_empty(), "flush should write to io");
        assert!(
            keys.iter().any(|k| k.ends_with(".chain")),
            "expected .chain file in flush output"
        );
    });

    // ── 4. Filtering ──────────────────────────────────────────────────

    check_async!("time_index since filter", {
        let store = FihStorage::new(SimIo::new(), "verify");
        AsyncFactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash::from_hex("f001"),
                origin: "v".into(),
                content: "data".into(),
                creator: "v".into(),
            },
        )
        .await
        .unwrap();
        let filter = StateFilter {
            since: Some("0".to_string()),
            ..Default::default()
        };
        let state = AsyncFilterCapable::read_state_filtered(&store, &filter).await;
        assert_eq!(state.facts.len(), 1);
    });

    check_async!("time_index until filter (time travel)", {
        let store = FihStorage::new(SimIo::new(), "verify");
        AsyncFactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash::from_hex("f001"),
                origin: "v".into(),
                content: "data".into(),
                creator: "v".into(),
            },
        )
        .await
        .unwrap();
        let filter = StateFilter {
            until: Some("0".to_string()),
            ..Default::default()
        };
        let state = AsyncFilterCapable::read_state_filtered(&store, &filter).await;
        assert_eq!(state.facts.len(), 0, "time travel to epoch should be empty");
    });

    // ── 5. Eviction ───────────────────────────────────────────────────

    check_async!("evict_before removes old hints", {
        let store = FihStorage::new(SimIo::new(), "verify");
        AsyncHintCapable::submit_hint(
            &store,
            &Hint {
                id: FihHash::from_hex("h001"),
                content: "old hint".into(),
                creator: "v".into(),
            },
        )
        .await
        .unwrap();
        let removed = AsyncEvictCapable::evict_before(&store, "99999999999")
            .await
            .unwrap();
        assert!(removed > 0, "should evict the hint");
        let state = AsyncStorageRead::read_state(&store).await;
        assert_eq!(state.hints.len(), 0);
    });

    // ── 6. Ref count / orphan detection ───────────────────────────────

    check_async!("ref_count orphan detection via conclude", {
        let store = FihStorage::new(SimIo::new(), "verify");
        AsyncFactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash::from_hex("f_orphan"),
                origin: "v".into(),
                content: "orphan".into(),
                creator: "v".into(),
            },
        )
        .await
        .unwrap();
        AsyncFactCapable::submit_fact(
            &store,
            &Fact {
                id: FihHash::from_hex("f_refd"),
                origin: "v".into(),
                content: "refd".into(),
                creator: "v".into(),
            },
        )
        .await
        .unwrap();
        AsyncIntentCapable::submit_intent(
            &store,
            &Intent {
                id: FihHash::from_hex("i001"),
                from_facts: vec![FihHash::from_hex("f_refd")],
                description: "test".into(),
                creator: "v".into(),
                worker: None,
                to_fact_id: None,
                last_heartbeat_at: None,
                created_at: None,
                is_concluded: false,
                concluded_at: None,
            },
        )
        .await
        .unwrap();

        AsyncIntentCapable::claim_intent(&store, "i001", "a")
            .await
            .unwrap();
        AsyncIntentCapable::conclude_intent(&store, "i001", "done")
            .await
            .unwrap();

        let state = AsyncStorageRead::read_state(&store).await;
        assert_eq!(state.facts.len(), 3, "2 original + 1 conclusion");
        assert!(state.intents[0].is_concluded, "intent should be concluded");
    });

    // ── 7. IntentStatus state machine (sync) ──────────────────────────

    check!("intent_status compile-time transitions", {
        let submitted = intent_status::IntentStatus::Submitted;
        let claimed = submitted.try_claim("alice", 100).unwrap();
        assert!(
            matches!(&claimed, intent_status::IntentStatus::Claimed{ worker, .. } if worker == "alice")
        );
        let hb = claimed.try_heartbeat("alice", 200).unwrap();
        assert!(
            matches!(&hb, intent_status::IntentStatus::Claimed{ last_heartbeat_at, .. } if *last_heartbeat_at == 200)
        );
        let concluded = hb.try_conclude("f_result", 300).unwrap();
        assert!(
            matches!(&concluded, intent_status::IntentStatus::Concluded{ to_fact, .. } if to_fact == &FihHash::from_hex("f_result").to_string())
        );
        assert!(!concluded.is_active());
    });

    // ── Summary ───────────────────────────────────────────────────────

    let failed = total - passed;
    eprintln!();
    eprintln!("+-----------------------------------------------------------+");
    eprintln!("|  result: {passed:>2}/{total:<2} passed, {failed} failed");
    eprintln!("+-----------------------------------------------------------+");

    if failed > 0 {
        std::process::exit(1);
    }
}
