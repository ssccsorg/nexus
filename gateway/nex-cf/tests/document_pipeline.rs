// Document pipeline integration tests for gateway/nex-cf.
//
// These tests run under `cargo test --workspace` and verify the full
// document ingestion -> semantic search pipeline using FsIo (tempfile)
// and InMemoryBm25. No Cloudflare bindings required.
//
// The tests exercise the same generic `handle_path()` and
// `ingest_document()` functions that the production CF Worker uses,
// ensuring the pipeline logic is correct regardless of deployment target.

use nexus_gateway_nex_cf::cf_io::TextQuery;
use nexus_gateway_nex_cf::stores::bm25::InMemoryBm25;
use nexus_model::AsyncStorageRead;

// -- Tests -------------------------------------------------------------------

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
        &storage, doc, "gnn-paper",
    ));
    assert!(result.is_ok());

    let state = rt.block_on(storage.read_state());
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.facts[0].origin, "document:gnn-paper");

    let results = storage
        .semantic_search(&TextQuery { text: "Graph Neural".into() }, 5)
        .expect("search should succeed");
    assert!(!results.is_empty());
    assert!(results[0].1 > 0.5, "BM25 score: {}", results[0].1);

    let no_match = storage
        .semantic_search(&TextQuery { text: "quantum physics".into() }, 5)
        .expect("search should succeed");
    assert!(
        no_match.is_empty() || no_match[0].1.abs() < f32::EPSILON,
        "non-matching should return zero or empty"
    );

    let doc2 = "Transformer architectures use self-attention mechanisms \
                for sequence processing";
    rt.block_on(nexus_gateway_nex_cf::ingest_document(
        &storage, doc2, "transformer-paper",
    ))
    .expect("second ingest should succeed");
    let state2 = rt.block_on(storage.read_state());
    assert_eq!(state2.facts.len(), 2);

    let attn = storage
        .semantic_search(&TextQuery { text: "self-attention".into() }, 5)
        .expect("search should succeed");
    assert!(attn[0].1 > 0.5, "self-attention score: {}", attn[0].1);
}

#[test]
fn document_ingestion_empty_text_fails() {
    let tmp = tempfile::TempDir::new().unwrap();
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage = nex::FihStorage::with_clock(io, "test-empty", Box::new(nexus_model::SystemClock));

    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(nexus_gateway_nex_cf::ingest_document(&storage, "", "empty-doc"));
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
        &storage, text, "multi-para",
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
fn handle_path_round_trip() {
    let tmp = tempfile::TempDir::new().unwrap();
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage =
        nex::FihStorage::with_clock(io, "test-handle-path", Box::new(nexus_model::SystemClock));

    let rt = tokio::runtime::Runtime::new().unwrap();

    let (code, _, body) = rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/", &[]));
    assert_eq!(code, 200);
    assert_eq!(body, "nexus-cf");

    let (code, _, _) =
        rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/nonexistent", &[]));
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
    let (code, _, _) = rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/conclude", &qd));
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
        let (code, _, _) =
            rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, route, &[]));
        assert_eq!(code, 200, "route {route} should be handled");
    }
}

#[test]
fn ingest_document_large_paragraph_does_not_truncate() {
    let tmp = tempfile::TempDir::new().unwrap();
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage =
        nex::FihStorage::with_clock(io, "test-large", Box::new(nexus_model::SystemClock));
    storage.register_semantic_store(Box::new(InMemoryBm25::new()));

    let rt = tokio::runtime::Runtime::new().unwrap();

    let long_text = "Rust ".repeat(100);
    rt.block_on(nexus_gateway_nex_cf::ingest_document(
        &storage, &long_text, "long-para",
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
    let storage = nex::FihStorage::with_clock(
        io,
        "test-claim-404",
        Box::new(nexus_model::SystemClock),
    );

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

    let result = storage.semantic_search(&TextQuery { text: "test".into() }, 5);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("no semantic stores"),
        "error should mention no stores: {err}"
    );
}

#[test]
fn ingest_all_from_mock_io_finds_dot_llms_dot_md() {
    use std::collections::HashMap;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};

    use nex::io::{AsyncFileIo, IoFuture, WriteOp};

    struct MockDocIo {
        data: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    }
    impl MockDocIo {
        fn new() -> Self {
            let mut m = HashMap::new();
            m.insert(
                "_llms/projects/nexus/index.llms.md".into(),
                b"# Nexus\n\nNexus is a FIH blackboard system.".to_vec(),
            );
            m.insert(
                "_llms/projects/ssccs/overview.llms.md".into(),
                b"# SSCCS\n\nSemantic State Coordination System.".to_vec(),
            );
            m.insert(
                "_llms/README.md".into(),
                b"# README".to_vec(),
            );
            Self { data: Arc::new(Mutex::new(m)) }
        }
    }
    impl AsyncFileIo for MockDocIo {
        fn read<'a>(&'a self, path: &'a str) -> IoFuture<'a, Option<Vec<u8>>> {
            let m = self.data.lock().unwrap();
            Box::pin(std::future::ready(Ok(m.get(path).cloned())))
        }
        fn write<'a>(&'a self, _path: &'a str, _data: &'a [u8]) -> IoFuture<'a, ()> {
            Box::pin(std::future::ready(Ok(())))
        }
        fn list<'a>(&'a self, prefix: &'a str) -> IoFuture<'a, Vec<String>> {
            let m = self.data.lock().unwrap();
            let keys: Vec<String> = m.keys().filter(|k| k.starts_with(prefix)).cloned().collect();
            Box::pin(std::future::ready(Ok(keys)))
        }
        fn delete<'a>(&'a self, _path: &'a str) -> IoFuture<'a, ()> {
            Box::pin(std::future::ready(Ok(())))
        }
        fn apply_batch<'a>(&'a self, _ops: &'a [WriteOp]) -> IoFuture<'a, ()> {
            Box::pin(std::future::ready(Ok(())))
        }
    }

    let tmp = tempfile::TempDir::new().unwrap();
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage =
        nex::FihStorage::with_clock(io, "test-ingest-all", Box::new(nexus_model::SystemClock));
    storage.register_semantic_store(Box::new(InMemoryBm25::new()));

    let doc_io = MockDocIo::new();
    let (total, errors) = tokio::runtime::Runtime::new()
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
        .semantic_search(&TextQuery { text: "Nexus blackboard".into() }, 5)
        .expect("search");
    assert!(!r.is_empty(), "nexus doc should be findable");

    let r = storage
        .semantic_search(&TextQuery { text: "SSCCS coordination".into() }, 5)
        .expect("search");
    assert!(!r.is_empty(), "ssccs doc should be findable");
}
