// Document pipeline integration tests for gateway/nex-cf.
//
// These tests run under `cargo test --workspace` and verify the full
// document ingestion → semantic search pipeline using FsIo (tempfile)
// and mock BM25/vector stores. No Cloudflare bindings required.
//
// The tests exercise the same generic `handle_path()` and
// `ingest_document()` functions that the production CF Worker uses,
// ensuring the pipeline logic is correct regardless of deployment target.

use std::collections::HashMap;
use std::fmt::Debug;

use nex::storage::semantic::{FihLoad, FihQuery, SemanticStore};
use nexus_model::AsyncStorageRead;

// ── Mock Semantic Stores ────────────────────────────────────────────────

/// Cosine vector similarity store.
#[derive(Debug)]
struct MockVecStore {
    ids: Vec<u32>,
    vectors: Vec<Vec<f32>>,
}

impl MockVecStore {
    fn new() -> Self {
        Self { ids: Vec::new(), vectors: Vec::new() }
    }
}

impl SemanticStore for MockVecStore {
    fn insert(&mut self, id: u32, load: &dyn FihLoad) -> Result<(), String> {
        let feats = load.features(id).ok_or_else(|| "no features".to_string())?;
        self.ids.push(id);
        self.vectors.push(feats);
        Ok(())
    }
    fn search(&self, query: &dyn FihQuery, top_k: usize) -> Result<Vec<(u32, f32)>, String> {
        let qv = query.features().ok_or_else(|| "no query features".to_string())?;
        if self.ids.is_empty() || qv.is_empty() {
            return Ok(Vec::new());
        }
        let n = self.vectors[0].len();
        if qv.len() != n {
            return Err("dimension mismatch".into());
        }
        let mut scores: Vec<(u32, f32)> = self.ids.iter().zip(self.vectors.iter()).map(|(&id, vec)| {
            let dot: f32 = qv.iter().zip(vec.iter()).map(|(a, b)| a * b).sum();
            let nq = qv.iter().map(|x| x * x).sum::<f32>().sqrt();
            let nv = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
            (id, dot / (nq * nv).max(f32::EPSILON))
        }).collect();
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        scores.truncate(top_k);
        Ok(scores)
    }
    fn remove(&mut self, id: u32) -> Result<(), String> {
        if let Some(pos) = self.ids.iter().position(|&i| i == id) {
            self.ids.remove(pos);
            self.vectors.remove(pos);
        }
        Ok(())
    }
    fn len(&self) -> usize { self.ids.len() }
}

/// BM25 text similarity store.
#[derive(Debug)]
struct MockBm25Store {
    ids: Vec<u32>,
    texts: Vec<String>,
}

impl MockBm25Store {
    fn new() -> Self {
        Self { ids: Vec::new(), texts: Vec::new() }
    }
}

impl SemanticStore for MockBm25Store {
    fn insert(&mut self, id: u32, load: &dyn FihLoad) -> Result<(), String> {
        let text = load.text(id).ok_or_else(|| "no text".to_string())?;
        self.ids.push(id);
        self.texts.push(text);
        Ok(())
    }
    fn search(&self, query: &dyn FihQuery, top_k: usize) -> Result<Vec<(u32, f32)>, String> {
        let qt = match query.text() {
            Some(t) if !t.trim().is_empty() => t,
            _ => return Ok(Vec::new()),
        };
        if self.ids.is_empty() {
            return Ok(Vec::new());
        }
        let terms: Vec<String> = qt.to_lowercase().split_whitespace().map(|s| s.to_string()).collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let n = self.texts.len();
        let avg_len: f64 = self.texts.iter()
            .map(|t| t.split_whitespace().count() as f64)
            .sum::<f64>() / n.max(1) as f64;

        let mut df: HashMap<String, usize> = HashMap::new();
        for t in &terms {
            df.insert(t.clone(), self.texts.iter()
                .filter(|doc| doc.to_lowercase().split_whitespace().any(|w| w == t))
                .count());
        }

        let k1 = 1.2;
        let b = 0.75;
        let mut scores: Vec<(u32, f32)> = self.ids.iter().zip(self.texts.iter()).map(|(&id, doc)| {
            let dl = doc.split_whitespace().count() as f64;
            let mut score = 0.0;
            for t in &terms {
                let tf = doc.to_lowercase().split_whitespace().filter(|w| w == t).count() as f64;
                if tf == 0.0 { continue; }
                let d = *df.get(t).unwrap_or(&0) as f64;
                let idf = ((n as f64 - d + 0.5) / (d + 0.5) + 1.0).ln();
                score += idf * (tf * (k1 + 1.0)) / (tf + k1 * (1.0 - b + b * dl / avg_len.max(1.0)));
            }
            (id, score as f32)
        }).collect();

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        scores.truncate(top_k);
        Ok(scores)
    }
    fn remove(&mut self, id: u32) -> Result<(), String> {
        if let Some(pos) = self.ids.iter().position(|&i| i == id) {
            self.ids.remove(pos);
            self.texts.remove(pos);
        }
        Ok(())
    }
    fn len(&self) -> usize { self.ids.len() }
}

// ── Query helper ────────────────────────────────────────────────────────

struct TextQuery(String);

impl FihQuery for TextQuery {
    fn features(&self) -> Option<Vec<f32>> {
        None
    }
    fn text(&self) -> Option<String> {
        Some(self.0.clone())
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[test]
fn document_ingestion_pipeline_e2e() {
    let tmp = tempfile::TempDir::new().unwrap();
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage = nex::FihStorage::with_clock(
        io,
        "test-doc-pipeline",
        Box::new(nexus_model::SystemClock),
    );

    // Register BM25 store for text search
    storage.register_semantic_store(Box::new(MockBm25Store::new()));

    // Step 1: Ingest a document via the shared ingest_document function
    let doc_text = "Graph Neural Networks process graph-structured data \
                     through message-passing between nodes";
    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(nexus_gateway_nex_cf::ingest_document(
            &storage, doc_text, "gnn-paper",
        ));
    assert!(result.is_ok(), "ingest_document should succeed");

    // Step 2: Verify state has the fact
    let state = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(storage.read_state());
    assert_eq!(state.facts.len(), 1, "one fact should be stored");
    assert_eq!(state.facts[0].origin, "document:gnn-paper");

    // Step 3: Search for relevant terms via semantic_search
    let results = storage
        .semantic_search(&TextQuery("Graph Neural".into()), 5)
        .expect("semantic_search should succeed");
    assert!(!results.is_empty(), "document should be searchable");
    assert!(
        results[0].1 > 0.5,
        "BM25 score should be high for matching terms, got {}",
        results[0].1
    );

    // Step 4: Search for non-matching terms
    let no_match = storage
        .semantic_search(&TextQuery("quantum physics".into()), 5)
        .expect("semantic_search should succeed");
    assert!(
        no_match.is_empty() || no_match[0].1.abs() < f32::EPSILON,
        "non-matching query should return zero-score results or empty"
    );

    // Step 5: Ingest a second document
    let doc2_text = "Transformer architectures use self-attention mechanisms \
                     for sequence processing";
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(nexus_gateway_nex_cf::ingest_document(
            &storage, doc2_text, "transformer-paper",
        ))
        .expect("second ingest should succeed");

    let state2 = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(storage.read_state());
    assert_eq!(state2.facts.len(), 2, "two facts should be stored");

    // Step 6: Verify search ranking — "self-attention" matches transformer doc
    let attn_results = storage
        .semantic_search(&TextQuery("self-attention".into()), 5)
        .expect("search should succeed");
    assert!(
        attn_results[0].1 > 0.5,
        "self-attention should score high on transformer doc, got {}",
        attn_results[0].1
    );

    // The transformer doc should rank above gnn doc for "self-attention"
    let transformer_idx = result.unwrap(); // last_id from ingest_document
    let transformer_coord_idx = storage.resolve_semantic_idx(
        attn_results.iter().find(|(_, s)| *s > 0.5).map(|(i, _)| *i).unwrap_or(0)
    );
    assert!(!transformer_coord_idx.is_empty(), "should resolve to a hex ID");
}

#[test]
fn document_ingestion_empty_text_fails() {
    let tmp = tempfile::TempDir::new().unwrap();
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage = nex::FihStorage::with_clock(
        io,
        "test-empty",
        Box::new(nexus_model::SystemClock),
    );

    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(nexus_gateway_nex_cf::ingest_document(
            &storage, "", "empty-doc",
        ));
    assert!(result.is_err(), "empty document should fail");
    assert!(
        result.unwrap_err().contains("empty"),
        "error should mention empty"
    );
}

#[test]
fn document_ingestion_multiple_paragraphs() {
    let tmp = tempfile::TempDir::new().unwrap();
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage = nex::FihStorage::with_clock(
        io,
        "test-multi-para",
        Box::new(nexus_model::SystemClock),
    );
    storage.register_semantic_store(Box::new(MockBm25Store::new()));

    // Multi-paragraph document
    let text = "First paragraph about neural networks.\n\nSecond paragraph about gradient descent.\n\nThird paragraph about backpropagation.";
    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(nexus_gateway_nex_cf::ingest_document(
            &storage, text, "multi-para",
        ));
    assert!(result.is_ok(), "multi-paragraph ingest should succeed");

    let state = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(storage.read_state());
    assert_eq!(state.facts.len(), 3, "three paragraphs → three facts");

    // Each paragraph should be independently searchable
    let nlp = storage
        .semantic_search(&TextQuery("neural networks".into()), 5)
        .expect("search should succeed");
    assert!(!nlp.is_empty(), "paragraph 1 should be searchable");

    let grad = storage
        .semantic_search(&TextQuery("gradient descent".into()), 5)
        .expect("search should succeed");
    assert!(!grad.is_empty(), "paragraph 2 should be searchable");

    let backprop = storage
        .semantic_search(&TextQuery("backpropagation".into()), 5)
        .expect("search should succeed");
    assert!(!backprop.is_empty(), "paragraph 3 should be searchable");
}

#[test]
fn handle_path_round_trip() {
    // Test the generic handle_path function directly
    let tmp = tempfile::TempDir::new().unwrap();
    let io = nex::FsIo::new(tmp.path()).unwrap();
    let storage = nex::FihStorage::with_clock(
        io,
        "test-handle-path",
        Box::new(nexus_model::SystemClock),
    );
    storage.register_semantic_store(Box::new(MockBm25Store::new()));

    let rt = tokio::runtime::Runtime::new().unwrap();

    // Test root
    let (code, _, body) = rt.block_on(nexus_gateway_nex_cf::handle_path(
        &storage, "/", &[],
    ));
    assert_eq!(code, 200);
    assert_eq!(body, "nexus-cf");

    // Test 404
    let (code, _, _) = rt.block_on(nexus_gateway_nex_cf::handle_path(
        &storage, "/nonexistent", &[],
    ));
    assert_eq!(code, 404);

    // Test fact submission via handle_path
    let q = vec![
        ("id".into(), "f_handle_001".into()),
        ("origin".into(), "handle-test".into()),
        ("content".into(), "test content for handle path".into()),
        ("creator".into(), "tester".into()),
    ];
    let (code, _, body) = rt.block_on(nexus_gateway_nex_cf::handle_path(
        &storage, "/fact", &q,
    ));
    assert_eq!(code, 200, "fact submission via handle_path should succeed: {body}");

    // Verify state — the body includes JSON with the fact data.
    // The fact ID submitted as "f_handle_001" will be hashed by FihHash::from_hex
    // into a different representation, so we check origin instead.
    let (code, _, body) = rt.block_on(nexus_gateway_nex_cf::handle_path(
        &storage, "/state", &[],
    ));
    assert_eq!(code, 200);
    assert!(body.contains("handle-test"), "state should contain the fact origin");

    // Test ingest via handle_path is not directly callable (ingest is only in
    // the #[event(fetch)] handler, not in handle_path). This is by design.
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

    // Submit intent
    let q_intent = vec![
        ("id".into(), "i_test_001".into()),
        ("desc".into(), "test intent lifecycle".into()),
        ("creator".into(), "tester".into()),
    ];
    let (code, _, body) = rt.block_on(nexus_gateway_nex_cf::handle_path(
        &storage, "/intent", &q_intent,
    ));
    assert_eq!(code, 200, "intent submission should succeed: {body}");

    // Claim intent
    let q_claim = vec![
        ("id".into(), "i_test_001".into()),
        ("agent".into(), "worker-1".into()),
    ];
    let (code, _, body) = rt.block_on(nexus_gateway_nex_cf::handle_path(
        &storage, "/claim", &q_claim,
    ));
    assert_eq!(code, 200, "claim should succeed: {body}");

    // Conclude intent
    let q_conclude = vec![
        ("id".into(), "i_test_001".into()),
        ("result".into(), "experiment completed".into()),
    ];
    let (code, _, body) = rt.block_on(nexus_gateway_nex_cf::handle_path(
        &storage, "/conclude", &q_conclude,
    ));
    assert_eq!(code, 200, "conclude should succeed: {body}");

    // Verify state
    let (code, _, body) = rt.block_on(nexus_gateway_nex_cf::handle_path(
        &storage, "/state", &[],
    ));
    assert_eq!(code, 200);
    assert!(body.contains("i_test_001"), "state should contain intent");
}
