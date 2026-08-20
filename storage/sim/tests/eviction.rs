// ── Eviction durability tests ───────────────────────────────────────────
//
// Eviction removes records from io, not just from the in-memory stores:
// read_state and later flushes must agree with the eviction. Hint
// submitted_at and intent created_at are second-granularity, so a
// FakeClock controls old vs new records unambiguously.

mod common;

use common::FakeClock;
use futures_executor::block_on;
use nex_fih::{
    AsyncEvictCapable, AsyncFactCapable, AsyncHintCapable, AsyncIntentCapable, AsyncStorageRead,
    Content, CoordId, Fact, FihStorage, Hint, Intent,
};
use nexus_storage_sim::{EntityStore, FileIo, SimIo};

/// Clock start at exactly t=100s, far from the next second boundary even
/// after the per-call step increments of the fake clock.
const START_NS: u64 = 100_000_000_000;

fn fact(id: &str) -> Fact {
    Fact::with_id(
        CoordId::resolve(id),
        "evict".into(),
        Content {
            mime_type: "text/plain".into(),
            data: id.as_bytes().to_vec(),
        },
        "t".into(),
    )
}

fn hint(id: &str, content: &str) -> Hint {
    Hint {
        id: CoordId::resolve(id),
        content: content.into(),
        creator: "t".into(),
    }
}

fn intent(id: &str) -> Intent {
    Intent {
        id: CoordId::resolve(id),
        from_facts: vec![CoordId::resolve("f_base")],
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

#[test]
fn evict_before_removes_only_old_hints() {
    block_on(async {
        let io = SimIo::new();
        let clock = FakeClock::new(START_NS);
        let advance = clock.clone();
        let store = FihStorage::with_clock(io.clone(), "evict", Box::new(clock));

        store.submit_hint(&hint("h_old", "old")).await.unwrap(); // t=100s
        advance.advance_secs(1);
        store.submit_hint(&hint("h_new", "new")).await.unwrap(); // t=101s

        // Cutoff at t=101s: only h_old is evicted; h_new survives.
        let removed = store
            .evict_before(&(START_NS / 1_000_000_000 + 1).to_string())
            .await
            .unwrap();
        assert_eq!(removed, 1);
        assert_eq!(store.hint_records.borrow().len(), 1);

        let state = store.read_state().await;
        assert_eq!(state.hints.len(), 1);
        assert_eq!(state.hints[0].id, CoordId::resolve("h_new"));

        store.flush_pending().await.unwrap();
        let keys = io.list("hints/").await.unwrap();
        assert_eq!(keys.len(), 1, "only the surviving hint stays on io");
        assert!(keys[0].starts_with("hints/h_"));
    });
}

#[test]
fn evict_before_deletes_io_records() {
    block_on(async {
        let io = SimIo::new();
        let store = FihStorage::new(io.clone(), "evict");
        store.submit_hint(&hint("h_a", "a")).await.unwrap();
        store.submit_hint(&hint("h_b", "b")).await.unwrap();

        let removed = store.evict_before(&u64::MAX.to_string()).await.unwrap();
        assert_eq!(removed, 2);
        assert_eq!(store.hint_records.borrow().len(), 0);

        let state = store.read_state().await;
        assert_eq!(state.hints.len(), 0);

        store.flush_pending().await.unwrap();
        let keys = io.list("hints/").await.unwrap();
        assert!(
            keys.is_empty(),
            "evicted hint files must be deleted from io"
        );
    });
}

#[test]
fn evict_stale_intents_removes_old_submitted() {
    block_on(async {
        let io = SimIo::new();
        let clock = FakeClock::new(START_NS);
        let advance = clock.clone();
        let store = FihStorage::with_clock(io.clone(), "evict", Box::new(clock));

        store.submit_fact(&fact("f_base")).await.unwrap();
        store.submit_intent(&intent("i_old")).await.unwrap(); // created_at = 100s
        advance.advance_secs(2);

        // older_than = 1s: cutoff = 102 - 1 = 101 > 100, so i_old is stale.
        let removed = store.evict_stale_intents(1).await.unwrap();
        assert_eq!(removed, 1);
        assert_eq!(store.intent_records.borrow().len(), 0);

        let state = store.read_state().await;
        assert_eq!(state.intents.len(), 0);

        store.flush_pending().await.unwrap();
        let keys = io.list("intents/").await.unwrap();
        assert!(
            keys.is_empty(),
            "evicted intent files must be deleted from io"
        );
    });
}
