// Record-layer content hash conflict detection (#176).
//
// The record layer keys facts by id. The id carries about 40 bits of
// content-derived entropy (documented on CoordId::content_id), so two
// distinct contents can collide on one id. A second fact at the same id
// is accepted only when it is the identical content (an idempotent
// retry); a different content_hash is rejected with Conflict instead of
// silently overwriting the earlier record.

use futures_executor::block_on;
use nex_fih::core::store::{Record, record_to_path};
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
        let err = store.submit_fact(&fact("f_a", b"beta")).await.unwrap_err();
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
            store.submit_fact(&fact("f_c", b"original")).await.unwrap();
            store.flush_pending().await.unwrap();
        }
        {
            // After rebuild the record map is empty; the id index built by
            // the reopened placement must still catch the conflict.
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

// A direct writer (nex-calc style) places a fact via place_record without
// submit_fact. The id index must expose it, so a submit_fact with the same
// id and different content is still rejected.
#[test]
fn conflict_detected_against_direct_writer_record() {
    block_on(async {
        let store = FihStorage::new(SimIo::new(), "direct-writer");
        let f = fact("f_g", b"direct-original");
        let id_str = f.id.to_string();
        let path = record_to_path(0u16, "", "user", 0u16, &id_str, 0, &f.content_hash);
        store.place_record(
            &path,
            Record::Fact {
                content: f.content.clone(),
                content_hash: f.content_hash,
                origin: f.origin.clone(),
                creator: f.creator.clone(),
                submitted_at: 0,
            },
        );
        let err = store
            .submit_fact(&fact("f_g", b"direct-different"))
            .await
            .unwrap_err();
        assert!(matches!(err, BlackboardError::Conflict(_)));
    });
}

// The idempotent counterpart: a direct-writer record with the identical
// content is accepted as a retry.
#[test]
fn idempotent_against_direct_writer_record() {
    block_on(async {
        let store = FihStorage::new(SimIo::new(), "direct-idem");
        let f = fact("f_h", b"direct-same");
        let id_str = f.id.to_string();
        let path = record_to_path(0u16, "", "user", 0u16, &id_str, 0, &f.content_hash);
        store.place_record(
            &path,
            Record::Fact {
                content: f.content.clone(),
                content_hash: f.content_hash,
                origin: f.origin.clone(),
                creator: f.creator.clone(),
                submitted_at: 0,
            },
        );
        let id = store.submit_fact(&f).await.unwrap();
        assert_eq!(id, f.id);
        // The idempotent path is a no-op: it must not add a record to the
        // same-session record map. (Direct-writer records live in the
        // unified store, not in the read-path entity stores.)
        assert_eq!(store.fact_records.borrow().len(), 0);
    });
}
