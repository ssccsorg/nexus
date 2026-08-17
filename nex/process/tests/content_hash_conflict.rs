// Record-layer content hash conflict detection (#176).
//
// The record layer keys facts by id. The id carries about 40 bits of
// content-derived entropy (documented on CoordId::content_id), so two
// distinct contents can collide on one id. A second fact at the same id
// is accepted only when it is the identical content (an idempotent
// retry); a different content_hash is rejected with Conflict instead of
// silently overwriting the earlier record.

use futures_executor::block_on;
use nex_fih::{
    AsyncFactCapable, AsyncStorageRead, BlackboardError, Content, CoordId, Fact, FihStorage,
};
use nexus_storage_sim::SimIo;

fn fact(id: &str, data: &[u8]) -> Fact {
    Fact::with_id(
        CoordId::resolve(id),
        "conflict".into(),
        Content {
            mime_type: "text/plain".into(),
            data: data.to_vec(),
        },
        "t".into(),
    )
}

#[test]
fn same_id_different_content_is_rejected() {
    block_on(async {
        let store = FihStorage::new(SimIo::new(), "conflict");
        store.submit_fact(&fact("f_a", b"alpha")).await.unwrap();
        let err = store
            .submit_fact(&fact("f_a", b"beta"))
            .await
            .unwrap_err();
        assert!(
            matches!(err, BlackboardError::Conflict(_)),
            "expected Conflict, got {err:?}"
        );
        // The earlier record is untouched.
        let state = store.read_state().await;
        assert_eq!(state.facts.len(), 1);
        assert_eq!(state.facts[0].content.data.as_slice(), b"alpha");
    });
}

#[test]
fn same_id_same_content_is_idempotent() {
    block_on(async {
        let store = FihStorage::new(SimIo::new(), "idem");
        let f = fact("f_b", b"same");
        let first = store.submit_fact(&f).await.unwrap();
        let second = store.submit_fact(&f).await.unwrap();
        assert_eq!(first, second);
        let state = store.read_state().await;
        assert_eq!(state.facts.len(), 1, "duplicate must not add a record");
    });
}

#[test]
fn conflict_is_detected_after_reopen() {
    block_on(async {
        let io = SimIo::new();
        {
            let store = FihStorage::new(io.clone(), "reopen-conflict");
            store
                .submit_fact(&fact("f_c", b"original"))
                .await
                .unwrap();
            store.flush_pending().await.unwrap();
        }
        {
            // After rebuild the record map is empty; the 19-axis store
            // fallback must still catch the conflict.
            let store = FihStorage::new(io, "reopen-conflict");
            store.rebuild_cache().await.unwrap();
            let err = store
                .submit_fact(&fact("f_c", b"different"))
                .await
                .unwrap_err();
            assert!(matches!(err, BlackboardError::Conflict(_)));
        }
    });
}
