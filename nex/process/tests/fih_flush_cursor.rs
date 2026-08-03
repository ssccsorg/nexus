// flush_since over durable io (#167).
//
// The delta is counted from the records on io, not from the pending
// buffer, so a reopened store catches up with a stale cursor. Each
// non-empty export writes a delta chain file that the ssccsdocs delta
// sync reads after restart.

use futures_executor::block_on;
use nex_fih::{
    AsyncFactCapable, AsyncFlushCapable, AsyncHintCapable, ChainEntry, Content, CoordId, Fact,
    FihStorage, FlushCursor, Hint,
};
use nexus_storage_sim::{FileIo, SimIo};

fn fact(id: &str, data: &str) -> Fact {
    Fact::with_id(
        CoordId::from_string(id),
        "test".into(),
        Content {
            mime_type: "text/plain".into(),
            data: data.as_bytes().to_vec(),
        },
        "tester".into(),
    )
}

fn hint(id: &str, content: &str) -> Hint {
    Hint {
        id: CoordId::from_string(id),
        content: content.into(),
        creator: "tester".into(),
    }
}

fn cursor(last_flushed_at: u64) -> FlushCursor {
    FlushCursor {
        last_flushed_at,
        partition: "default".into(),
    }
}

#[test]
fn flush_since_counts_durable_records_after_reopen() {
    block_on(async {
        let io = SimIo::new();
        // Session 1: ingest and flush without acking a cursor.
        {
            let store = FihStorage::new(io.clone(), "durable");
            store.submit_fact(&fact("f_a", "a")).await.unwrap();
            store.submit_fact(&fact("f_b", "b")).await.unwrap();
            store.flush_pending().await.unwrap();
        }
        // Session 2: a stale cursor catches up with the durable records.
        {
            let store = FihStorage::new(io.clone(), "durable");
            let first = store.flush_since(&cursor(0)).await.unwrap();
            assert_eq!(
                first.records_flushed, 2,
                "durable records since the stale cursor must be counted"
            );
            // The acked delta does not repeat.
            let second = store.flush_since(&first.new_cursor).await.unwrap();
            assert_eq!(second.records_flushed, 0);
        }
    });
}

#[test]
fn flush_since_writes_delta_chain() {
    block_on(async {
        let io = SimIo::new();
        let store = FihStorage::new(io.clone(), "chain");
        store.submit_fact(&fact("f_chain", "data")).await.unwrap();
        store
            .submit_hint(&hint("h_chain", "ephemeral"))
            .await
            .unwrap();
        let result = store.flush_since(&cursor(0)).await.unwrap();
        // Hints are ephemeral: the delta counts facts and intents only.
        assert_eq!(result.records_flushed, 1);

        let keys = io.list("flush/").await.unwrap();
        let chain_key = keys
            .iter()
            .find(|k| k.ends_with(".chain"))
            .expect("delta chain file");
        assert!(chain_key.starts_with("flush/default/"));
        let bytes = io.read(chain_key).await.unwrap().expect("chain bytes");
        let entry: ChainEntry = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(entry.prev_cursor, 0);
        assert_eq!(entry.records_flushed, 1);
        assert_eq!(entry.facts.len(), 1);
        assert!(entry.intents.is_empty());
    });
}

#[test]
fn flush_since_exports_only_new_delta() {
    block_on(async {
        let io = SimIo::new();
        let store = FihStorage::new(io.clone(), "delta");
        store.submit_fact(&fact("f_old", "old")).await.unwrap();
        let first = store.flush_since(&cursor(0)).await.unwrap();
        assert_eq!(first.records_flushed, 1);

        // A fact submitted after the cursor is the only new delta.
        store.submit_fact(&fact("f_new", "new")).await.unwrap();
        let second = store.flush_since(&first.new_cursor).await.unwrap();
        assert_eq!(second.records_flushed, 1);

        let third = store.flush_since(&second.new_cursor).await.unwrap();
        assert_eq!(third.records_flushed, 0);
    });
}

#[test]
fn flush_since_empty_delta_writes_no_chain() {
    block_on(async {
        let io = SimIo::new();
        let store = FihStorage::new(io.clone(), "empty");
        let result = store.flush_since(&cursor(u64::MAX)).await.unwrap();
        assert_eq!(result.records_flushed, 0);
        let keys = io.list("flush/").await.unwrap();
        assert!(keys.is_empty(), "no delta means no chain file");
    });
}
