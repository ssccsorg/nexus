// Record-layer content hash conflict detection (#176).
//
// The record layer keys facts by id. Since the CoordId migration the
// id injectively encodes the full content hash, so distinct contents
// collide on one id only up to SHA-256 collision resistance (2^128
// birthday). The conflict guard is kept as defense-in-depth: a second
// fact at the same id is accepted only when it is the identical content
// (an idempotent retry); a different content_hash is rejected with
// Conflict instead of silently overwriting the earlier record.

use futures_executor::block_on;
use nex_fih::FihId20 as _;
use nex_fih::{AsyncFactCapable, BlackboardError, Content, CoordId, Fact};
use nexus_storage_ref::{FactRecord, RefFihStorage};
use nex_io::SimIo;

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
        let store = RefFihStorage::new(SimIo::new(), "conflict");
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
        let store = RefFihStorage::new(SimIo::new(), "idem");
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
            let store = RefFihStorage::new(io.clone(), "reopen-conflict");
            store.submit_fact(&fact("f_c", b"original")).await.unwrap();
            store.flush_pending().await.unwrap();
        }
        {
            // After rebuild the record map is empty; the id index built by
            // the reopened placement must still catch the conflict.
            let store = RefFihStorage::new(io, "reopen-conflict");
            store.rebuild_cache().await.unwrap();
            let err = store
                .submit_fact(&fact("f_c", b"different"))
                .await
                .unwrap_err();
            assert!(matches!(err, BlackboardError::Conflict(_)));
        }
    });
}

// A record present directly in the record map (bypassing submit_fact)
// must still guard the id: the pre-check reads the same map, so a
// submit_fact with a different content_hash is rejected and the earlier
// record survives. The commit-point insert-return detector stays as
// defense-in-depth (single-threaded, so the pre-check and the insert
// cannot diverge in practice).
#[test]
fn conflict_detected_for_fact_store_only_record() {
    block_on(async {
        let store = RefFihStorage::new(SimIo::new(), "fstore-only");
        let f = fact("f_i", b"fstore-original");
        store.fact_records.borrow_mut().insert(
            f.id,
            FactRecord {
                id: f.id.to_string(),
                blob_hash: f.content_hash.to_string(),
                origin: f.origin.clone(),
                creator: f.creator.clone(),
                submitted_at: 0,
            },
        );
        let err = store
            .submit_fact(&fact("f_i", b"fstore-different"))
            .await
            .unwrap_err();
        assert!(matches!(err, BlackboardError::Conflict(_)));
        // The earlier record must be preserved, not overwritten.
        let state = store.fact_records.borrow().get(&f.id).cloned().unwrap();
        assert_eq!(state.blob_hash, f.content_hash.to_string());
    });
}

// The idempotent counterpart: a record-map-only record with identical
// content is a no-op retry that does not add a second record.
#[test]
fn idempotent_for_fact_store_only_record() {
    block_on(async {
        let store = RefFihStorage::new(SimIo::new(), "fstore-idem");
        let f = fact("f_j", b"fstore-same");
        store.fact_records.borrow_mut().insert(
            f.id,
            FactRecord {
                id: f.id.to_string(),
                blob_hash: f.content_hash.to_string(),
                origin: f.origin.clone(),
                creator: f.creator.clone(),
                submitted_at: 0,
            },
        );
        let id = store.submit_fact(&f).await.unwrap();
        assert_eq!(id, f.id);
        assert_eq!(
            store.fact_records.borrow().len(),
            1,
            "the record-map-only record stays; the retry adds nothing"
        );
    });
}
