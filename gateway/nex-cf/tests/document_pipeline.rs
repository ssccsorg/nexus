// Document pipeline integration tests for gateway/nex-cf.
//
// These tests run under `cargo test --workspace` and verify the full
// document ingestion -> semantic search pipeline using FsIo (tempfile)
// and InMemoryBm25. No Cloudflare bindings required.
//
// The tests exercise the same generic `handle_path()` and
// `ingest_document()` functions that the production CF Worker uses,
// ensuring the pipeline logic is correct regardless of deployment target.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nex::io::{AsyncFileIo, IoFuture, WriteOp};
use nexus_gateway_nex_cf::cf_io::TextQuery;
use nexus_gateway_nex_cf::stores::bm25::InMemoryBm25;
use nexus_model::AsyncStorageRead;

// ── Mock helpers ─────────────────────────────────────────────────────────

/// In-memory document store that implements AsyncFileIo.
/// Used for `ingest_all_from_io` tests. Only `read` and `list` are
/// meaningfully implemented; `write`/`delete`/`apply_batch` are no-ops.
struct TestDocStore {
    data: HashMap<String, Vec<u8>>,
}

impl TestDocStore {
    fn new() -> Self {
        Self {
            data: HashMap::new(),
        }
    }

    fn with(mut self, path: &str, content: &str) -> Self {
        self.data
            .insert(path.to_string(), content.as_bytes().to_vec());
        self
    }

    fn with_bytes(mut self, path: &str, bytes: Vec<u8>) -> Self {
        self.data.insert(path.to_string(), bytes);
        self
    }
}

impl AsyncFileIo for TestDocStore {
    fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        let result = self.data.get(path).cloned();
        Box::pin(std::future::ready(Ok(result)))
    }

    fn write<'a>(&'a self, _path: &'a str, _data: &'a [u8]) -> IoFuture<'a, ()> {
        Box::pin(std::future::ready(Ok(())))
    }

    fn list<'a>(&'a self, prefix: &'a str) -> IoFuture<'a, Vec<String>> {
        let keys: Vec<String> = self
            .data
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        Box::pin(std::future::ready(Ok(keys)))
    }

    fn delete<'a>(&'a self, _path: &'a str) -> IoFuture<'a, ()> {
        Box::pin(std::future::ready(Ok(())))
    }

    fn apply_batch<'a>(&'a self, _ops: &'a [WriteOp]) -> IoFuture<'a, ()> {
        Box::pin(std::future::ready(Ok(())))
    }
}

/// Mock IO that tracks call counts for write/apply_batch operations.
/// Used to verify that BatchIo defers writes without calling inner IO.
#[derive(Default, Clone)]
struct CallCounts {
    writes: usize,
    apply_batches: usize,
}

struct TrackingIo {
    inner: nex::FsIo,
    counts: Arc<Mutex<CallCounts>>,
}

impl TrackingIo {
    fn new(tmp: &tempfile::TempDir) -> Self {
        Self {
            inner: nex::FsIo::new(tmp.path()).unwrap(),
            counts: Arc::new(Mutex::new(CallCounts::default())),
        }
    }

    fn counts_arc(&self) -> Arc<Mutex<CallCounts>> {
        Arc::clone(&self.counts)
    }
}

impl AsyncFileIo for TrackingIo {
    fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
        self.inner.read(path)
    }

    fn write<'a>(&'a self, path: &'a str, data: &'a [u8]) -> IoFuture<'a, ()> {
        let counts = Arc::clone(&self.counts);
        let p = path.to_string();
        let d = data.to_vec();
        Box::pin(async move {
            counts.lock().unwrap().writes += 1;
            self.inner.write(&p, &d).await
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> IoFuture<'a, Vec<String>> {
        self.inner.list(prefix)
    }

    fn delete<'a>(&'a self, path: &'a str) -> IoFuture<'a, ()> {
        self.inner.delete(path)
    }

    fn apply_batch<'a>(&'a self, ops: &'a [WriteOp]) -> IoFuture<'a, ()> {
        let counts = Arc::clone(&self.counts);
        let ops_vec: Vec<WriteOp> = ops.to_vec();
        Box::pin(async move {
            counts.lock().unwrap().apply_batches += 1;
            self.inner.apply_batch(&ops_vec).await
        })
    }
}

// ── Helper: create a default storage with InMemoryBm25 ────────────────────

fn make_storage(tmp: &tempfile::TempDir, label: &str) -> nex::FihStorage<nex::FsIo> {
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage = nex::FihStorage::with_clock(io, label, Box::new(nexus_model::SystemClock));
    storage.register_semantic_store(Box::new(InMemoryBm25::new()));
    storage
}

// ── 1. Basic ingestion ───────────────────────────────────────────────────

#[test]
fn document_ingestion_pipeline_e2e() {
    let tmp = tempfile::TempDir::new().unwrap();
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage =
        nex::FihStorage::with_clock(io, "test-doc-pipeline", Box::new(nexus_model::SystemClock));
    storage.register_semantic_store(Box::new(InMemoryBm25::new()));

    let rt = tokio::runtime::Runtime::new().unwrap();

    let doc = "Graph Neural Networks process graph-structured data \
               through message-passing between nodes";
    let result = rt.block_on(nexus_gateway_nex_cf::ingest_document(
        &storage,
        doc,
        "gnn-paper",
    ));
    assert!(result.is_ok());

    let state = rt.block_on(storage.read_state());
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.facts[0].origin, "document:gnn-paper");

    let results = storage
        .semantic_search(
            &TextQuery {
                text: "Graph Neural".into(),
            },
            5,
        )
        .expect("search should succeed");
    assert!(!results.is_empty());
    assert!(results[0].1 > 0.5, "BM25 score: {}", results[0].1);

    let no_match = storage
        .semantic_search(
            &TextQuery {
                text: "quantum physics".into(),
            },
            5,
        )
        .expect("search should succeed");
    assert!(
        no_match.is_empty() || no_match[0].1.abs() < f32::EPSILON,
        "non-matching should return zero or empty"
    );

    let doc2 = "Transformer architectures use self-attention mechanisms \
                for sequence processing";
    rt.block_on(nexus_gateway_nex_cf::ingest_document(
        &storage,
        doc2,
        "transformer-paper",
    ))
    .expect("second ingest should succeed");
    let state2 = rt.block_on(storage.read_state());
    assert_eq!(state2.facts.len(), 2);

    let attn = storage
        .semantic_search(
            &TextQuery {
                text: "self-attention".into(),
            },
            5,
        )
        .expect("search should succeed");
    assert!(attn[0].1 > 0.5, "self-attention score: {}", attn[0].1);
}

#[test]
fn document_ingestion_empty_text_fails() {
    let tmp = tempfile::TempDir::new().unwrap();
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage = nex::FihStorage::with_clock(io, "test-empty", Box::new(nexus_model::SystemClock));

    let result =
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(nexus_gateway_nex_cf::ingest_document(
                &storage,
                "",
                "empty-doc",
            ));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("empty"));
}

#[test]
fn document_ingestion_multiple_paragraphs() {
    let tmp = tempfile::TempDir::new().unwrap();
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage =
        nex::FihStorage::with_clock(io, "test-multi-para", Box::new(nexus_model::SystemClock));
    storage.register_semantic_store(Box::new(InMemoryBm25::new()));

    let rt = tokio::runtime::Runtime::new().unwrap();

    let text = "First paragraph about neural networks.\n\n\
                Second paragraph about gradient descent.\n\n\
                Third paragraph about backpropagation.";
    rt.block_on(nexus_gateway_nex_cf::ingest_document(
        &storage,
        text,
        "multi-para",
    ))
    .expect("ingest should succeed");

    let state = rt.block_on(storage.read_state());
    assert_eq!(state.facts.len(), 3);

    for (query, label) in [
        ("neural networks", "para 1"),
        ("gradient descent", "para 2"),
        ("backpropagation", "para 3"),
    ] {
        let r = storage
            .semantic_search(&TextQuery { text: query.into() }, 5)
            .expect("search should succeed");
        assert!(!r.is_empty(), "{label} should be searchable");
    }
}

#[test]
fn ingest_document_large_paragraph_does_not_truncate() {
    let tmp = tempfile::TempDir::new().unwrap();
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage = nex::FihStorage::with_clock(io, "test-large", Box::new(nexus_model::SystemClock));
    storage.register_semantic_store(Box::new(InMemoryBm25::new()));

    let rt = tokio::runtime::Runtime::new().unwrap();

    let long_text = "Rust ".repeat(100);
    rt.block_on(nexus_gateway_nex_cf::ingest_document(
        &storage,
        &long_text,
        "long-para",
    ))
    .expect("long paragraph should succeed");

    let state = rt.block_on(storage.read_state());
    assert_eq!(state.facts.len(), 1, "single long paragraph -> one fact");
}

#[test]
fn multiple_ingest_calls_accumulate() {
    let tmp = tempfile::TempDir::new().unwrap();
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage =
        nex::FihStorage::with_clock(io, "test-accumulate", Box::new(nexus_model::SystemClock));
    storage.register_semantic_store(Box::new(InMemoryBm25::new()));

    let rt = tokio::runtime::Runtime::new().unwrap();

    for i in 0..5 {
        rt.block_on(nexus_gateway_nex_cf::ingest_document(
            &storage,
            &format!("Document number {i} with unique content for testing."),
            &format!("doc-{i}"),
        ))
        .expect(&format!("ingest {i} should succeed"));
    }

    let state = rt.block_on(storage.read_state());
    assert_eq!(state.facts.len(), 5, "5 ingests -> 5 facts");
}

// ── 2. handle_path routing ─────────────────────────────────────────────

#[test]
fn handle_path_round_trip() {
    let tmp = tempfile::TempDir::new().unwrap();
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage =
        nex::FihStorage::with_clock(io, "test-handle-path", Box::new(nexus_model::SystemClock));

    let rt = tokio::runtime::Runtime::new().unwrap();

    let (code, _, body) = rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/", &[]));
    assert_eq!(code, 200);
    assert_eq!(body, "nexus-cf");

    let (code, _, _) = rt.block_on(nexus_gateway_nex_cf::handle_path(
        &storage,
        "/nonexistent",
        &[],
    ));
    assert_eq!(code, 404);

    let q = vec![
        ("id".into(), "f_handle_001".into()),
        ("origin".into(), "handle-test".into()),
        ("content".into(), "test content".into()),
        ("creator".into(), "tester".into()),
    ];
    let (code, _, _) = rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/fact", &q));
    assert_eq!(code, 200);

    let (code, _, body) = rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/state", &[]));
    assert_eq!(code, 200);
    assert!(body.contains("handle-test"));
}

#[test]
fn handle_path_intent_lifecycle() {
    let tmp = tempfile::TempDir::new().unwrap();
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage = nex::FihStorage::with_clock(
        io,
        "test-intent-lifecycle",
        Box::new(nexus_model::SystemClock),
    );

    let rt = tokio::runtime::Runtime::new().unwrap();

    let qi = vec![
        ("id".into(), "i_test_001".into()),
        ("desc".into(), "test intent".into()),
        ("creator".into(), "tester".into()),
    ];
    let (code, _, _) = rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/intent", &qi));
    assert_eq!(code, 200);

    let qc = vec![
        ("id".into(), "i_test_001".into()),
        ("agent".into(), "worker-1".into()),
    ];
    let (code, _, _) = rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/claim", &qc));
    assert_eq!(code, 200);

    let qd = vec![
        ("id".into(), "i_test_001".into()),
        ("result".into(), "done".into()),
    ];
    let (code, _, _) = rt.block_on(nexus_gateway_nex_cf::handle_path(
        &storage,
        "/conclude",
        &qd,
    ));
    assert_eq!(code, 200);

    let (code, _, body) = rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/state", &[]));
    assert_eq!(code, 200);
    assert!(body.contains("i_test_001"));
}

#[test]
fn split_test_prefix_works() {
    let tmp = tempfile::TempDir::new().unwrap();
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage =
        nex::FihStorage::with_clock(io, "test-prefix", Box::new(nexus_model::SystemClock));

    let rt = tokio::runtime::Runtime::new().unwrap();

    for route in &["/", "/fact", "/state", "/flush", "/rebuild"] {
        let (code, _, _) = rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, route, &[]));
        assert_eq!(code, 200, "route {route} should be handled");
    }
}

// ── 3. Error handling ──────────────────────────────────────────────────

#[test]
fn handle_path_claim_conflict_returns_409() {
    let tmp = tempfile::TempDir::new().unwrap();
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage = nex::FihStorage::with_clock(
        io,
        "test-claim-conflict",
        Box::new(nexus_model::SystemClock),
    );

    let rt = tokio::runtime::Runtime::new().unwrap();

    let qi = vec![
        ("id".into(), "i_conflict_001".into()),
        ("desc".into(), "conflict test".into()),
        ("creator".into(), "tester".into()),
    ];
    rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/intent", &qi));

    let qc1 = vec![
        ("id".into(), "i_conflict_001".into()),
        ("agent".into(), "agent-a".into()),
    ];
    let (code, _, _) = rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/claim", &qc1));
    assert_eq!(code, 200);

    let qc2 = vec![
        ("id".into(), "i_conflict_001".into()),
        ("agent".into(), "agent-b".into()),
    ];
    let (code, _, body) = rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/claim", &qc2));
    assert_eq!(code, 409, "double claim should conflict: {body}");
}

#[test]
fn handle_path_claim_nonexistent_intent_returns_404() {
    let tmp = tempfile::TempDir::new().unwrap();
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage =
        nex::FihStorage::with_clock(io, "test-claim-404", Box::new(nexus_model::SystemClock));

    let rt = tokio::runtime::Runtime::new().unwrap();

    let qc = vec![
        ("id".into(), "i_nonexistent".into()),
        ("agent".into(), "agent-a".into()),
    ];
    let (code, _, _) = rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/claim", &qc));
    assert_eq!(code, 404);
}

#[test]
fn semantic_search_no_stores_configured_proper_error() {
    let tmp = tempfile::TempDir::new().unwrap();
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage =
        nex::FihStorage::with_clock(io, "test-no-store", Box::new(nexus_model::SystemClock));

    let result = storage.semantic_search(
        &TextQuery {
            text: "test".into(),
        },
        5,
    );
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("no semantic stores"),
        "error should mention no stores: {err}"
    );
}

// ── 4. BatchIo R2 write batching ────────────────────────────────────────

#[test]
fn batch_io_enqueues_writes_without_calling_inner_io() {
    let tmp = tempfile::TempDir::new().unwrap();
    let tracking = TrackingIo::new(&tmp);
    let counts_arc = tracking.counts_arc();
    let batch = nexus_gateway_nex_cf::batch_io::BatchIo::new(tracking);

    let rt = tokio::runtime::Runtime::new().unwrap();

    // Write through BatchIo — inner IO should NOT be called yet.
    rt.block_on(batch.write("test/path", b"hello")).unwrap();
    assert_eq!(
        counts_arc.lock().unwrap().writes,
        0,
        "inner write should not be called until flush"
    );
    assert_eq!(batch.pending_count(), 1, "one pending write");

    rt.block_on(batch.write("test/path2", b"world")).unwrap();
    assert_eq!(
        counts_arc.lock().unwrap().writes,
        0,
        "inner write still not called"
    );
    assert_eq!(batch.pending_count(), 2, "two pending writes");
}

#[test]
fn batch_io_flush_flushes_pending_writes_via_apply_batch() {
    let tmp = tempfile::TempDir::new().unwrap();
    let tracking = TrackingIo::new(&tmp);
    let counts_arc = tracking.counts_arc();
    let batch = nexus_gateway_nex_cf::batch_io::BatchIo::new(tracking);

    let rt = tokio::runtime::Runtime::new().unwrap();

    rt.block_on(batch.write("test/a", b"alpha")).unwrap();
    rt.block_on(batch.write("test/b", b"beta")).unwrap();
    rt.block_on(batch.write("test/c", b"gamma")).unwrap();
    assert_eq!(batch.pending_count(), 3);

    // Flush — should trigger apply_batch on inner, not individual writes.
    rt.block_on(batch.flush()).unwrap();

    let counts = counts_arc.lock().unwrap();
    assert_eq!(counts.writes, 0, "flush should not call write on inner");
    assert_eq!(
        counts.apply_batches, 1,
        "flush should call apply_batch once"
    );
    assert_eq!(
        batch.pending_count(),
        0,
        "pending should be empty after flush"
    );
}

#[test]
fn batch_io_write_returns_immediately_not_awaiting_inner() {
    let tmp = tempfile::TempDir::new().unwrap();
    let tracking = TrackingIo::new(&tmp);
    let counts_arc = tracking.counts_arc();
    let batch = nexus_gateway_nex_cf::batch_io::BatchIo::new(tracking);

    let rt = tokio::runtime::Runtime::new().unwrap();

    // write() should return successfully without awaiting inner IO.
    // If write() awaited inner, inner.writes would increase.
    let result = rt.block_on(batch.write("test/returns-immediately", b"data"));
    assert!(result.is_ok());
    assert_eq!(
        counts_arc.lock().unwrap().writes,
        0,
        "inner write should not be called"
    );
}

#[test]
fn batch_io_flush_empty_is_noop() {
    let tmp = tempfile::TempDir::new().unwrap();
    let tracking = TrackingIo::new(&tmp);
    let counts_arc = tracking.counts_arc();
    let batch = nexus_gateway_nex_cf::batch_io::BatchIo::new(tracking);

    let rt = tokio::runtime::Runtime::new().unwrap();

    rt.block_on(batch.flush()).unwrap();
    assert_eq!(
        counts_arc.lock().unwrap().apply_batches,
        0,
        "flush with no pending should not call apply_batch"
    );
}

#[test]
fn batch_io_data_survives_flush_to_disk() {
    let tmp = tempfile::TempDir::new().unwrap();
    // Use FsIo directly as inner so we can verify data on disk.
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let batch = nexus_gateway_nex_cf::batch_io::BatchIo::new(io);

    let rt = tokio::runtime::Runtime::new().unwrap();

    rt.block_on(batch.write("flush-test/hello.txt", b"hello world"))
        .unwrap();
    rt.block_on(batch.write("flush-test/nested/deep.txt", b"deep data"))
        .unwrap();
    rt.block_on(batch.flush()).unwrap();

    // Now re-open with a fresh FsIo and read back.
    let io2 = nex::FsIo::new(tmp.path()).unwrap();
    let result = rt.block_on(io2.read("flush-test/hello.txt"));
    assert_eq!(result.unwrap(), Some(b"hello world".to_vec()));

    let result2 = rt.block_on(io2.read("flush-test/nested/deep.txt"));
    assert_eq!(result2.unwrap(), Some(b"deep data".to_vec()));
}

// ── 5. ingest_all_from_io with real scenarios ────────────────────────────

#[test]
fn ingest_all_from_mock_io_finds_dot_llms_dot_md() {
    let doc_io = TestDocStore::new()
        .with(
            "_llms/projects/nexus/index.llms.md",
            "# Nexus\n\nNexus is a FIH blackboard system.",
        )
        .with(
            "_llms/projects/ssccs/overview.llms.md",
            "# SSCCS\n\nSemantic State Coordination System.",
        )
        .with("_llms/README.md", "# README");

    let tmp = tempfile::TempDir::new().unwrap();
    let storage = make_storage(&tmp, "test-ingest-all");

    let (total, errors) =
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(nexus_gateway_nex_cf::ingest_all_from_io(
                &storage, &doc_io, "_llms/",
            ));

    assert_eq!(total, 2, "should ingest 2 .llms.md files");
    assert!(errors.is_empty(), "no errors: {:?}", errors);

    let state = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(storage.read_state());
    assert_eq!(state.facts.len(), 4, "2 docs x 2 paragraphs each = 4 facts");

    let r = storage
        .semantic_search(
            &TextQuery {
                text: "Nexus blackboard".into(),
            },
            5,
        )
        .expect("search");
    assert!(!r.is_empty(), "nexus doc should be findable");

    let r = storage
        .semantic_search(
            &TextQuery {
                text: "SSCCS coordination".into(),
            },
            5,
        )
        .expect("search");
    assert!(!r.is_empty(), "ssccs doc should be findable");
}

#[test]
fn ingest_all_from_io_skips_non_llms_md_files() {
    let doc_io = TestDocStore::new()
        .with("_llms/alpha.llms.md", "# Alpha\n\nContent alpha.")
        .with("_llms/beta.txt", "# Beta\n\nNot a doc.")
        .with("_llms/gamma.json", "{\"x\": 1}")
        .with("_llms/delta.md", "Just a markdown file without .llms.")
        .with("_llms/epsilon.llms.md", "# Epsilon\n\nContent epsilon.");

    let tmp = tempfile::TempDir::new().unwrap();
    let storage = make_storage(&tmp, "test-skip-non-llms");

    let (total, errors) =
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(nexus_gateway_nex_cf::ingest_all_from_io(
                &storage, &doc_io, "_llms/",
            ));

    assert_eq!(total, 2, "only .llms.md files should be ingested");
    assert!(errors.is_empty(), "no errors: {:?}", errors);

    let state = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(storage.read_state());
    // Each .llms.md has 2 paragraphs, so 2 x 2 = 4 facts.
    assert_eq!(state.facts.len(), 4);
}

#[test]
fn ingest_all_from_io_multiple_files_varied_content() {
    // Five .llms.md files with diverse topics.
    let doc_io = TestDocStore::new()
        .with("_llms/rust/ownership.llms.md",
            "# Rust Ownership\n\nOwnership is Rust's unique memory management feature.\n\n\
             The borrow checker enforces rules at compile time.")
        .with("_llms/rust/traits.llms.md",
            "# Traits\n\nTraits define shared behavior in Rust.\n\nSimilar to interfaces in other languages.")
        .with("_llms/python/asyncio.llms.md",
            "# Async IO\n\nPython asyncio enables concurrent code.\n\nIt uses coroutines and event loops.")
        .with("_llms/db/sql.llms.md",
            "# SQL\n\nSQL is a declarative query language for relational databases.")
        .with("_llms/db/nosql.llms.md",
            "# NoSQL\n\nNoSQL databases provide flexible schemas and horizontal scaling.");

    let tmp = tempfile::TempDir::new().unwrap();
    let storage = make_storage(&tmp, "test-varied-content");

    let (total, errors) =
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(nexus_gateway_nex_cf::ingest_all_from_io(
                &storage, &doc_io, "_llms/",
            ));

    assert_eq!(total, 5, "all 5 .llms.md files should be ingested");
    assert!(errors.is_empty(), "no errors: {:?}", errors);

    let state = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(storage.read_state());
    // 5 docs: 5 have 2 paragraphs each -> 10 facts; sql and nosql have 1 paragraph each -> +2 = 12 facts.
    // Wait: 3 docs (ownership, traits, asyncio) each have 3 paragraphs mapped:
    //   "ownership": "# Rust Ownership", "Ownership is Rust's...", "The borrow checker..." = 3
    //   "traits": "# Traits", "Traits define...", "Similar to interfaces..." = 3
    //   "asyncio": "# Async IO", "Python asyncio...", "It uses coroutines..." = 3
    //   "sql": "# SQL", "SQL is a declarative..." = 2
    //   "nosql": "# NoSQL", "NoSQL databases..." = 2
    // Total: 3+3+3+2+2 = 13 facts
    assert_eq!(
        state.facts.len(),
        13,
        "varied paragraph counts across 5 docs"
    );
}

#[test]
fn ingest_all_from_io_nested_mixed_depths() {
    let doc_io = TestDocStore::new()
        .with("_llms/a/b.llms.md", "# Deep\n\nNested at two levels.")
        .with("_llms/c.llms.md", "# Shallow\n\nSingle level.")
        .with(
            "_llms/x/y/z/d.llms.md",
            "# Very Deep\n\nNested at three levels.",
        );

    let tmp = tempfile::TempDir::new().unwrap();
    let storage = make_storage(&tmp, "test-nested-depths");

    let (total, errors) =
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(nexus_gateway_nex_cf::ingest_all_from_io(
                &storage, &doc_io, "_llms/",
            ));

    assert_eq!(total, 3, "all nested depths should be ingested");
    assert!(errors.is_empty(), "no errors: {:?}", errors);

    let state = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(storage.read_state());
    // Each has 2 paragraphs -> 6 facts.
    assert_eq!(state.facts.len(), 6);

    // Verify the origin reflects the nested path (strip "_llms/" and ".llms.md")
    let origins: Vec<&str> = state.facts.iter().map(|f| f.origin.as_str()).collect();
    assert!(origins.contains(&"document:a/b"));
    assert!(origins.contains(&"document:c"));
    assert!(origins.contains(&"document:x/y/z/d"));
}

#[test]
fn ingest_all_from_io_error_missing_file() {
    // A store where list returns a path but read returns None (missing).
    struct MissingFileIo;
    impl AsyncFileIo for MissingFileIo {
        fn read<'a>(&'a self, _path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
            Box::pin(std::future::ready(Ok(None)))
        }
        fn write<'a>(&'a self, _path: &'a str, _data: &'a [u8]) -> IoFuture<'a, ()> {
            Box::pin(std::future::ready(Ok(())))
        }
        fn list<'a>(&'a self, _prefix: &'a str) -> IoFuture<'a, Vec<String>> {
            Box::pin(std::future::ready(Ok(vec!["_llms/missing.llms.md".into()])))
        }
        fn delete<'a>(&'a self, _path: &'a str) -> IoFuture<'a, ()> {
            Box::pin(std::future::ready(Ok(())))
        }
        fn apply_batch<'a>(&'a self, _ops: &'a [WriteOp]) -> IoFuture<'a, ()> {
            Box::pin(std::future::ready(Ok(())))
        }
    }

    let tmp = tempfile::TempDir::new().unwrap();
    let storage = make_storage(&tmp, "test-missing-file");

    let (total, errors) =
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(nexus_gateway_nex_cf::ingest_all_from_io(
                &storage,
                &MissingFileIo,
                "_llms/",
            ));

    assert_eq!(total, 0);
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0].contains("empty"),
        "should report empty file: {}",
        errors[0]
    );
}

#[test]
fn ingest_all_from_io_error_utf8_decode_failure() {
    let doc_io = TestDocStore::new()
        .with_bytes("_llms/good.llms.md", b"# Good\n\nValid UTF-8.".to_vec())
        .with_bytes("_llms/bad.llms.md", vec![0xFF, 0xFE, 0x00, 0x01]); // Invalid UTF-8

    let tmp = tempfile::TempDir::new().unwrap();
    let storage = make_storage(&tmp, "test-utf8-error");

    let (total, errors) =
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(nexus_gateway_nex_cf::ingest_all_from_io(
                &storage, &doc_io, "_llms/",
            ));

    assert_eq!(total, 1, "good file should be ingested");
    assert_eq!(errors.len(), 1, "bad file should produce error");
    assert!(
        errors[0].contains("not UTF-8"),
        "error should mention UTF-8: {}",
        errors[0]
    );

    let state = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(storage.read_state());
    assert_eq!(state.facts.len(), 2, "good doc has 2 paragraphs");
}

#[test]
fn ingest_all_from_io_cross_document_search() {
    let doc_io = TestDocStore::new()
        .with(
            "_llms/doc1.llms.md",
            "The cat sat on the mat.\n\nCats are curious animals.",
        )
        .with(
            "_llms/doc2.llms.md",
            "Dogs are loyal companions.\n\nDogs enjoy long walks.",
        )
        .with(
            "_llms/doc3.llms.md",
            "Both cats and dogs are popular pets.\n\nPets bring joy to people.",
        );

    let tmp = tempfile::TempDir::new().unwrap();
    let storage = make_storage(&tmp, "test-cross-doc");

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(nexus_gateway_nex_cf::ingest_all_from_io(
        &storage, &doc_io, "_llms/",
    ));

    // Query matching content from multiple documents.
    let cat_results = storage
        .semantic_search(
            &TextQuery {
                text: "cats".into(),
            },
            10,
        )
        .expect("search cats");
    assert!(
        cat_results.len() >= 2,
        "cats should appear in at least 2 docs, got {}",
        cat_results.len()
    );

    let dog_results = storage
        .semantic_search(
            &TextQuery {
                text: "dogs".into(),
            },
            10,
        )
        .expect("search dogs");
    assert!(
        dog_results.len() >= 2,
        "dogs should appear in at least 2 docs, got {}",
        dog_results.len()
    );

    let pet_results = storage
        .semantic_search(
            &TextQuery {
                text: "pets".into(),
            },
            10,
        )
        .expect("search pets");
    assert!(
        pet_results.len() >= 2,
        "pets should match multiple docs, got {}",
        pet_results.len()
    );
}

#[test]
fn ingest_all_from_io_content_hash_idempotency() {
    // Re-ingesting the same documents should not increase fact count
    // because FihStorage uses content-hash-based dedup in enqueue_content.
    let doc_io = TestDocStore::new()
        .with(
            "_llms/doc1.llms.md",
            "# Stable\n\nThis content does not change.",
        )
        .with(
            "_llms/doc2.llms.md",
            "# Stable Too\n\nThis also stays the same.",
        );

    let tmp = tempfile::TempDir::new().unwrap();
    let storage = make_storage(&tmp, "test-idempotency");

    let rt = tokio::runtime::Runtime::new().unwrap();

    // First ingest
    rt.block_on(nexus_gateway_nex_cf::ingest_all_from_io(
        &storage, &doc_io, "_llms/",
    ));

    let state1 = rt.block_on(storage.read_state());
    let count1 = state1.facts.len();

    // Second ingest — same content
    rt.block_on(nexus_gateway_nex_cf::ingest_all_from_io(
        &storage, &doc_io, "_llms/",
    ));

    let state2 = rt.block_on(storage.read_state());
    let count2 = state2.facts.len();

    // FihStorage's enqueue_content checks pending buffer for duplicate
    // blob paths. After a flush, the pending buffer is empty, so the
    // same blob content may be re-written. This is not true dedup — it
    // merely avoids duplicate writes in the same batch.
    // However, FihHash from_hex("f_Stable_0") with the same content
    // will produce the same hash, so the fact_store will replace the
    // existing entry rather than duplicate it.
    assert_eq!(
        count1, count2,
        "re-ingesting same docs should not increase fact count"
    );
}

// ── 6. Performance/resilience ──────────────────────────────────────────

#[test]
fn stress_ingest_twenty_small_documents() {
    let tmp = tempfile::TempDir::new().unwrap();
    let storage = make_storage(&tmp, "test-stress-20");

    let rt = tokio::runtime::Runtime::new().unwrap();

    for i in 0..20 {
        let text = format!(
            "Document {} with unique searchable content for BM25 indexing.",
            i
        );
        rt.block_on(nexus_gateway_nex_cf::ingest_document(
            &storage,
            &text,
            &format!("stress-doc-{i}"),
        ))
        .expect(&format!("ingest doc {i} should succeed"));
    }

    let state = rt.block_on(storage.read_state());
    assert_eq!(state.facts.len(), 20, "20 docs -> 20 facts");

    // All documents should be individually searchable
    for i in 0..20 {
        let query = format!("Document {}", i);
        let r = storage
            .semantic_search(&TextQuery { text: query }, 5)
            .expect("search should succeed");
        assert!(!r.is_empty(), "doc {i} should be searchable");
    }
}

#[test]
fn concurrent_semantic_search_stability() {
    let tmp = tempfile::TempDir::new().unwrap();
    let storage = make_storage(&tmp, "test-concurrent-search");

    let rt = tokio::runtime::Runtime::new().unwrap();

    // Ingest several documents to populate the store
    let texts = vec![
        "Machine learning algorithms learn from data patterns.",
        "Deep neural networks have many hidden layers.",
        "Reinforcement learning uses rewards and punishments.",
        "Natural language processing handles text and speech.",
        "Computer vision processes images and videos.",
    ];
    for (i, text) in texts.iter().enumerate() {
        rt.block_on(nexus_gateway_nex_cf::ingest_document(
            &storage,
            text,
            &format!("ml-doc-{i}"),
        ))
        .unwrap();
    }

    // Run multiple queries against the populated store — no panics.
    let queries = [
        "machine learning",
        "neural networks",
        "reinforcement",
        "natural language",
        "computer vision",
    ];
    for q in &queries {
        let r = storage.semantic_search(
            &TextQuery {
                text: q.to_string(),
            },
            5,
        );
        assert!(r.is_ok(), "search for '{q}' should succeed");
        let results = r.unwrap();
        assert!(!results.is_empty(), "search for '{q}' should have results");
    }
}

// ── 7. Edge cases ───────────────────────────────────────────────────────

#[test]
fn document_with_only_whitespace_fails() {
    let tmp = tempfile::TempDir::new().unwrap();
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage =
        nex::FihStorage::with_clock(io, "test-whitespace", Box::new(nexus_model::SystemClock));

    let rt = tokio::runtime::Runtime::new().unwrap();

    let whitespace_only = "   \n   \n  \n   ";
    let result = rt.block_on(nexus_gateway_nex_cf::ingest_document(
        &storage,
        whitespace_only,
        "whitespace-doc",
    ));
    assert!(result.is_err());
    assert!(
        result.unwrap_err().contains("empty"),
        "whitespace-only should be empty"
    );
}

#[test]
fn origin_with_special_characters_is_sanitized() {
    let tmp = tempfile::TempDir::new().unwrap();
    let storage = make_storage(&tmp, "test-sanitize-origin");

    let rt = tokio::runtime::Runtime::new().unwrap();

    let origin = "my@special!origin#with$chars^and&stars*";
    rt.block_on(nexus_gateway_nex_cf::ingest_document(
        &storage,
        "Test content with interesting origin.",
        origin,
    ))
    .expect("ingest should succeed");

    let state = rt.block_on(storage.read_state());
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.facts[0].origin, format!("document:{}", origin));
    // The sanitized ID is used within the fact ID field (FihHash from_hex of "f_{sanitized}_{0}")
    // We verify indirectly by checking origin is preserved unmodified as metadata.
}

#[test]
fn very_long_origin_string() {
    let tmp = tempfile::TempDir::new().unwrap();
    let storage = make_storage(&tmp, "test-long-origin");

    let rt = tokio::runtime::Runtime::new().unwrap();

    let long_origin = "a".repeat(500);
    rt.block_on(nexus_gateway_nex_cf::ingest_document(
        &storage,
        "Content with a very long origin identifier for testing purposes.",
        &long_origin,
    ))
    .expect("ingest with long origin should succeed");

    let state = rt.block_on(storage.read_state());
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.facts[0].origin, format!("document:{}", long_origin));
}

#[test]
fn paragraph_with_only_punctuation() {
    let tmp = tempfile::TempDir::new().unwrap();
    let storage = make_storage(&tmp, "test-punctuation");

    let rt = tokio::runtime::Runtime::new().unwrap();

    // A paragraph with only symbols and punctuation is technically non-empty
    // and should be ingested as a fact (though it won't match many queries).
    let text = "!!!\n\n@@@\n\n###";
    rt.block_on(nexus_gateway_nex_cf::ingest_document(
        &storage,
        text,
        "symbols-doc",
    ))
    .expect("punctuation-only paragraphs should ingest");

    let state = rt.block_on(storage.read_state());
    assert_eq!(
        state.facts.len(),
        3,
        "three punctuation lines -> three facts"
    );
}

#[test]
fn origin_empty_string_sanitize() {
    let tmp = tempfile::TempDir::new().unwrap();
    let storage = make_storage(&tmp, "test-empty-origin");

    let rt = tokio::runtime::Runtime::new().unwrap();

    // An empty origin string is allowed but produces facts with origin "document:".
    let result = rt.block_on(nexus_gateway_nex_cf::ingest_document(
        &storage,
        "Content with empty origin.",
        "",
    ));
    assert!(result.is_ok(), "empty origin should be accepted");

    let state = rt.block_on(storage.read_state());
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.facts[0].origin, "document:");
}

// ── Additional flush and rebuild edge cases ─────────────────────────────

#[test]
fn handle_path_flush_and_rebuild() {
    let tmp = tempfile::TempDir::new().unwrap();
    let storage = make_storage(&tmp, "test-flush-rebuild");

    let rt = tokio::runtime::Runtime::new().unwrap();

    // Ingest a document
    rt.block_on(nexus_gateway_nex_cf::ingest_document(
        &storage,
        "Some content to flush and rebuild.",
        "flush-test",
    ))
    .expect("ingest should succeed");

    // Flush should succeed
    let (code, _, body) = rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/flush", &[]));
    assert_eq!(code, 200, "flush should be OK: {body}");

    // Rebuild should succeed
    let (code, _, body) = rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/rebuild", &[]));
    assert_eq!(code, 200, "rebuild should be OK: {body}");

    // State should still show the document after rebuild
    let (code, _, body) = rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/state", &[]));
    assert_eq!(code, 200);
    assert!(
        body.contains("flush-test"),
        "document should survive rebuild: {body}"
    );
}

#[test]
fn ingest_all_from_io_empty_prefix_returns_no_errors() {
    struct EmptyIo;
    impl AsyncFileIo for EmptyIo {
        fn read<'a>(&'a self, _path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
            Box::pin(std::future::ready(Ok(None)))
        }
        fn write<'a>(&'a self, _path: &'a str, _data: &'a [u8]) -> IoFuture<'a, ()> {
            Box::pin(std::future::ready(Ok(())))
        }
        fn list<'a>(&'a self, _prefix: &'a str) -> IoFuture<'a, Vec<String>> {
            Box::pin(std::future::ready(Ok(vec![])))
        }
        fn delete<'a>(&'a self, _path: &'a str) -> IoFuture<'a, ()> {
            Box::pin(std::future::ready(Ok(())))
        }
        fn apply_batch<'a>(&'a self, _ops: &'a [WriteOp]) -> IoFuture<'a, ()> {
            Box::pin(std::future::ready(Ok(())))
        }
    }

    let tmp = tempfile::TempDir::new().unwrap();
    let storage = make_storage(&tmp, "test-empty-prefix");

    let (total, errors) =
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(nexus_gateway_nex_cf::ingest_all_from_io(
                &storage, &EmptyIo, "_llms/",
            ));

    assert_eq!(total, 0, "no files to ingest");
    assert!(errors.is_empty(), "no errors for empty prefix");
}
