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
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

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

/// An in-memory FileIo whose first write fails, then behaves normally.
/// Used to prove that a failed flush re-queues its ops instead of losing
/// them.
struct FlakyIo {
    map: Mutex<HashMap<String, Vec<u8>>>,
    fail_first_write: AtomicBool,
}

impl FlakyIo {
    fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
            fail_first_write: AtomicBool::new(true),
        }
    }
}

impl FileIo for FlakyIo {
    fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        let map = &self.map;
        Box::pin(async move { Ok(map.lock().unwrap().get(path).cloned()) })
    }

    fn write<'a>(&'a self, path: &'a str, data: &'a [u8]) -> IoFuture<'a, ()> {
        let should_fail = self.fail_first_write.swap(false, Ordering::SeqCst);
        let map = &self.map;
        Box::pin(async move {
            if should_fail {
                return Err(format!("injected write failure: {path}"));
            }
            map.lock().unwrap().insert(path.to_string(), data.to_vec());
            Ok(())
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> IoFuture<'a, Vec<String>> {
        let map = &self.map;
        Box::pin(async move {
            Ok(map
                .lock()
                .unwrap()
                .keys()
                .filter(|k| k.starts_with(prefix))
                .cloned()
                .collect())
        })
    }

    fn delete<'a>(&'a self, path: &'a str) -> IoFuture<'a, ()> {
        let map = &self.map;
        Box::pin(async move {
            map.lock().unwrap().remove(path);
            Ok(())
        })
    }
}

fn fact(id: &str) -> Fact {
    Fact::with_id(
        CoordId::resolve(id),
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
        id: CoordId::resolve(id),
        from_facts: vec![CoordId::resolve("f_base")],
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

/// Submit the base fact and an intent referencing it; submit only enqueues,
/// so this succeeds even with a failing IO backend.
async fn seed(storage: &FihStorage<FailingIo>) {
    storage.submit_fact(&fact("f_base")).await.unwrap();
    storage.submit_intent(&intent("i_flush")).await.unwrap();
}

#[test]
fn claim_flush_failure_is_internal_not_not_found() {
    block_on(async {
        let storage = FihStorage::new(FailingIo, "flush-error");
        seed(&storage).await;

        // The pre-read flush in claim fails, and the failure must surface
        // as Internal, not NotFound.
        let err = storage.claim_intent("i_flush", "worker").await.unwrap_err();
        assert!(matches!(err, BlackboardError::Internal(_)));
    });
}

#[test]
fn heartbeat_flush_failure_is_internal_not_not_found() {
    block_on(async {
        let storage = FihStorage::new(FailingIo, "flush-error");
        seed(&storage).await;

        let err = storage.heartbeat("i_flush", "worker").await.unwrap_err();
        assert!(matches!(err, BlackboardError::Internal(_)));
    });
}

#[test]
fn release_flush_failure_is_internal_not_not_found() {
    block_on(async {
        let storage = FihStorage::new(FailingIo, "flush-error");
        seed(&storage).await;

        let err = storage
            .release_intent("i_flush", "worker")
            .await
            .unwrap_err();
        assert!(matches!(err, BlackboardError::Internal(_)));
    });
}

#[test]
fn conclude_flush_failure_is_internal_not_not_found() {
    block_on(async {
        let storage = FihStorage::new(FailingIo, "flush-error");
        seed(&storage).await;

        let err = storage
            .conclude_intent("i_flush", "done")
            .await
            .unwrap_err();
        assert!(matches!(err, BlackboardError::Internal(_)));
    });
}

#[test]
fn failed_flush_requeues_ops_for_retry() {
    block_on(async {
        let storage = FihStorage::new(FlakyIo::new(), "retry");
        storage.submit_fact(&fact("f_base")).await.unwrap();
        storage.submit_intent(&intent("i_retry")).await.unwrap();

        // The first flush fails on the first write; the batch must be
        // re-queued, not lost.
        let err = storage.flush_pending().await.unwrap_err();
        assert!(err.contains("injected"), "got: {err}");

        // The retried flush applies the same ops, and the intent becomes
        // claimable: nothing was lost by the failed attempt.
        storage.flush_pending().await.unwrap();
        storage.claim_intent("i_retry", "worker").await.unwrap();
    });
}
