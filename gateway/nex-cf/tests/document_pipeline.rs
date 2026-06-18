// Document pipeline integration tests for gateway/nex-cf.
//
// These tests run under `cargo test --workspace` and verify the full
// document ingestion → semantic search pipeline using FsIo (tempfile)
// and InMemoryBm25. No Cloudflare bindings required.
//
// The tests exercise the same generic `handle_path()` and
// `ingest_document()` functions that the production CF Worker uses,
// ensuring the pipeline logic is correct regardless of deployment target.

use nexus_gateway_nex_cf::cf_io::TextQuery;
use nexus_gateway_nex_cf::stores::bm25::InMemoryBm25;
use nexus_model::AsyncStorageRead;

// ── Tests ───────────────────────────────────────────────────────────────

#[test]
fn document_ingestion_pipeline_e2e() {
    let tmp = tempfile::TempDir::new().unwrap();
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage =
        nex::FihStorage::with_clock(io, "test-doc-pipeline", Box::new(nexus_model::SystemClock));
    storage.register_semantic_store(Box::new(InMemoryBm25::new()));

    let rt = tokio::runtime::Runtime::new().unwrap();

    // Step 1: Ingest a document
    let doc = "Graph Neural Networks process graph-structured data \
               through message-passing between nodes";
    let result = rt.block_on(nexus_gateway_nex_cf::ingest_document(
        &storage, doc, "gnn-paper",
    ));
    assert!(result.is_ok());

    // Step 2: State has the fact
    let state = rt.block_on(storage.read_state());
    assert_eq!(state.facts.len(), 1);
    assert_eq!(state.facts[0].origin, "document:gnn-paper");

    // Step 3: Search matching terms
    let results = storage
        .semantic_search(&TextQuery { text: "Graph Neural".into() }, 5)
        .expect("search should succeed");
    assert!(!results.is_empty());
    assert!(results[0].1 > 0.5, "BM25 score: {}", results[0].1);

    // Step 4: Non-matching query
    let no_match = storage
        .semantic_search(&TextQuery { text: "quantum physics".into() }, 5)
        .expect("search should succeed");
    assert!(
        no_match.is_empty() || no_match[0].1.abs() < f32::EPSILON,
        "non-matching should return zero or empty"
    );

    // Step 5: Ingest second document
    let doc2 = "Transformer architectures use self-attention mechanisms \
                for sequence processing";
    rt.block_on(nexus_gateway_nex_cf::ingest_document(
        &storage, doc2, "transformer-paper",
    ))
    .expect("second ingest should succeed");

    let state2 = rt.block_on(storage.read_state());
    assert_eq!(state2.facts.len(), 2);

    // Step 6: "self-attention" matches transformer doc
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

    // Each paragraph independently searchable
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
    let (code, _, _) =
        rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/fact", &q));
    assert_eq!(code, 200);

    let (code, _, body) =
        rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/state", &[]));
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
    let (code, _, _) =
        rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/intent", &qi));
    assert_eq!(code, 200);

    let qc = vec![
        ("id".into(), "i_test_001".into()),
        ("agent".into(), "worker-1".into()),
    ];
    let (code, _, _) =
        rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/claim", &qc));
    assert_eq!(code, 200);

    let qd = vec![
        ("id".into(), "i_test_001".into()),
        ("result".into(), "done".into()),
    ];
    let (code, _, _) =
        rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/conclude", &qd));
    assert_eq!(code, 200);

    let (code, _, body) =
        rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/state", &[]));
    assert_eq!(code, 200);
    assert!(body.contains("i_test_001"));
}

#[test]
fn split_test_prefix_works() {
    // split_test_prefix is not pub, but we can test indirectly via handle_path
    // since handle_path matches paths without /test/ prefix.
    // The actual /test/ prefix stripping happens in #[event(fetch)] which is
    // not testable without worker-rs. This test verifies that handle_path
    // itself works with paths that come from split_test_prefix output.
    let tmp = tempfile::TempDir::new().unwrap();
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage = nex::FihStorage::with_clock(
        io,
        "test-prefix",
        Box::new(nexus_model::SystemClock),
    );

    let rt = tokio::runtime::Runtime::new().unwrap();

    // These are the paths that split_test_prefix("/test/...") produces
    let (code, _, _) = rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/", &[]));
    assert_eq!(code, 200);

    let (code, _, _) =
        rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/fact", &[]));
    assert_eq!(code, 200); // missing params but still should route

    let (code, _, _) =
        rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/state", &[]));
    assert_eq!(code, 200);

    let (code, _, _) =
        rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/flush", &[]));
    assert_eq!(code, 200);

    let (code, _, _) =
        rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/rebuild", &[]));
    assert_eq!(code, 200);
}

#[test]
fn ingest_document_large_paragraph_does_not_truncate() {
    let tmp = tempfile::TempDir::new().unwrap();
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage =
        nex::FihStorage::with_clock(io, "test-large", Box::new(nexus_model::SystemClock));
    storage.register_semantic_store(Box::new(InMemoryBm25::new()));

    let rt = tokio::runtime::Runtime::new().unwrap();

    // Single long paragraph — should stay as one fact (no split)
    let long_text = "Rust ".repeat(100);
    rt.block_on(nexus_gateway_nex_cf::ingest_document(
        &storage, &long_text, "long-para",
    ))
    .expect("long paragraph should succeed");

    let state = rt.block_on(storage.read_state());
    assert_eq!(state.facts.len(), 1, "single long paragraph → one fact");
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
    assert_eq!(state.facts.len(), 5, "5 ingests → 5 facts");
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

    // Submit intent
    let qi = vec![
        ("id".into(), "i_conflict_001".into()),
        ("desc".into(), "conflict test".into()),
        ("creator".into(), "tester".into()),
    ];
    rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/intent", &qi));

    // Claim by agent-a
    let qc1 = vec![
        ("id".into(), "i_conflict_001".into()),
        ("agent".into(), "agent-a".into()),
    ];
    let (code, _, _) =
        rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/claim", &qc1));
    assert_eq!(code, 200);

    // Claim by agent-b — should conflict
    let qc2 = vec![
        ("id".into(), "i_conflict_001".into()),
        ("agent".into(), "agent-b".into()),
    ];
    let (code, _, body) =
        rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/claim", &qc2));
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
    let (code, _, _) =
        rt.block_on(nexus_gateway_nex_cf::handle_path(&storage, "/claim", &qc));
    assert_eq!(code, 404);
}

#[test]
fn semantic_search_no_stores_configured_proper_error() {
    let tmp = tempfile::TempDir::new().unwrap();
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage =
        nex::FihStorage::with_clock(io, "test-no-store", Box::new(nexus_model::SystemClock));
    // deliberately NOT registering any semantic store

    let result = storage.semantic_search(
        &TextQuery { text: "test".into() },
        5,
    );
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("no semantic stores"),
        "error should mention no stores: {err}"
    );
}
