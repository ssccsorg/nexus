// FihStorage flush error surfacing.
//
// Intent operations flush pending writes before reading the intent record
// from IO. A failed flush must surface as BlackboardError::Internal, not
// be misreported as a missing intent.

use futures_executor::block_on;
use nex_fih::io::file_io::{FileIo, IoFuture};
use nex_fih::{
    AsyncFactCapable, AsyncIntentCapable, BlackboardError, Content, CoordId, Fact, FihStorage,
    Intent,
};

/// A FileIo backend whose writes always fail.
struct FailingIo;

impl FileIo for FailingIo {
    fn read<'a>(&'a self, _path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        Box::pin(async move { Ok(None) })
    }

    fn write<'a>(&'a self, path: &'a str, _data: &'a [u8]) -> IoFuture<'a, ()> {
        Box::pin(async move { Err(format!("write failed: {path}")) })
    }

    fn list<'a>(&'a self, _prefix: &'a str) -> IoFuture<'a, Vec<String>> {
        Box::pin(async move { Ok(Vec::new()) })
    }

    fn delete<'a>(&'a self, _path: &'a str) -> IoFuture<'a, ()> {
        Box::pin(async move { Ok(()) })
    }
}

fn fact(id: &str) -> Fact {
    Fact::with_id(
        CoordId::from_string(id),
        "test".into(),
        Content {
            mime_type: "text/plain".into(),
            data: b"base".to_vec(),
        },
        "tester".into(),
    )
}

fn intent(id: &str) -> Intent {
    Intent {
        id: CoordId::from_string(id),
        from_facts: vec![CoordId::from_string("f_base")],
        description: format!("intent {id}"),
        creator: "tester".into(),
        worker: None,
        to_fact_id: None,
        last_heartbeat_at: None,
        created_at: None,
        is_concluded: false,
        concluded_at: None,
    }
}

#[test]
fn claim_flush_failure_is_internal_not_not_found() {
    block_on(async {
        let storage = FihStorage::new(FailingIo, "flush-error");
        storage.submit_fact(&fact("f_base")).await.unwrap();
        storage.submit_intent(&intent("i_flush")).await.unwrap();

        // submit only enqueues; the pre-read flush in claim fails, and
        // the failure must surface as Internal, not NotFound.
        let err = storage.claim_intent("i_flush", "worker").await.unwrap_err();
        assert!(matches!(err, BlackboardError::Internal(_)));
    });
}

#[test]
fn heartbeat_flush_failure_is_internal_not_not_found() {
    block_on(async {
        let storage = FihStorage::new(FailingIo, "flush-error");
        storage.submit_fact(&fact("f_base")).await.unwrap();
        storage.submit_intent(&intent("i_flush")).await.unwrap();

        let err = storage.heartbeat("i_flush", "worker").await.unwrap_err();
        assert!(matches!(err, BlackboardError::Internal(_)));
    });
}
