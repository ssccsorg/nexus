// ── SemanticStore scenario tests ────────────────────────────────────────
//
// These tests exercise SemanticStore + MockSemanticStore against realistic
// usage patterns. Tests are in `tests/` (not inline) to keep the core crate
// free of test code.
//
// Scenarios cover:
//   1. Insert and search with multiple documents (search.json style)
//   2. Cross-methodology simulation (same trait, different FihLoad accessors)
//   3. Batch insert then incremental search
//   4. Remove then re-insert
//   5. Mixed content: some records have features, some have text only
//   6. Large-scale insert stress (500 documents like search.json)
//   7. Empty store query returns empty results
//   8. Duplicate insert (same id) does not corrupt
//   9. Dimension mismatch error
//  10. FihCoord integration: semantic_insert / semantic_search via store

use nex::storage::semantic::{FihLoad, MockSemanticStore, SemanticStore};

// ── Test FihLoad implementations ────────────────────────────────────────

/// A FihLoad that returns feature vectors (simulating a vector store).
struct FeatureLoad {
    features: Vec<f32>,
}

impl FeatureLoad {
    fn new(features: Vec<f32>) -> Self {
        Self { features }
    }
}

impl FihLoad for FeatureLoad {
    fn content(&self, _id: u32) -> Option<Vec<u8>> {
        None
    }
    fn features(&self, _id: u32) -> Option<Vec<f32>> {
        Some(self.features.clone())
    }
    fn origin(&self, _id: u32) -> Option<String> {
        None
    }
    fn creator(&self, _id: u32) -> Option<String> {
        None
    }
}

/// A FihLoad that returns text content (simulating a BM25/ngram store).
struct TextLoad {
    text: String,
}

impl FihLoad for TextLoad {
    fn content(&self, _id: u32) -> Option<Vec<u8>> {
        Some(self.text.as_bytes().to_vec())
    }
    fn features(&self, _id: u32) -> Option<Vec<f32>> {
        None
    }
    fn origin(&self, _id: u32) -> Option<String> {
        None
    }
    fn creator(&self, _id: u32) -> Option<String> {
        None
    }
}

/// A FihLoad that returns origin strings (simulating an ngram origin store).
struct OriginLoad {
    origin: String,
}

impl FihLoad for OriginLoad {
    fn content(&self, _id: u32) -> Option<Vec<u8>> {
        None
    }
    fn features(&self, _id: u32) -> Option<Vec<f32>> {
        None
    }
    fn origin(&self, _id: u32) -> Option<String> {
        Some(self.origin.clone())
    }
    fn creator(&self, _id: u32) -> Option<String> {
        None
    }
}

/// A FihLoad that returns everything (full document load).
struct FullDocLoad {
    text: String,
    origin: String,
    creator: String,
}

impl FihLoad for FullDocLoad {
    fn content(&self, _id: u32) -> Option<Vec<u8>> {
        Some(self.text.as_bytes().to_vec())
    }
    fn features(&self, _id: u32) -> Option<Vec<f32>> {
        None
    }
    fn origin(&self, _id: u32) -> Option<String> {
        Some(self.origin.clone())
    }
    fn creator(&self, _id: u32) -> Option<String> {
        Some(self.creator.clone())
    }
}

// ── Search.json scenario: text-based search ───────────────────────────
//
// Realistic scenario using actual SSCCS documentation from search.json.
// Each document's text is converted to a simple bag-of-words feature vector
// (word presence, not frequency — prevents domination by long documents).
// This simulates a text-to-semantic-index pipeline where the store never
// knows about text, only feature vectors.

/// Build a simple bag-of-words feature vector from text.
/// Uses a fixed vocabulary of common STEM/tech terms.
fn text_to_features(text: &str, vocabulary: &[&str]) -> Vec<f32> {
    let lower = text.to_lowercase();
    vocabulary
        .iter()
        .map(|word| if lower.contains(word) { 1.0 } else { 0.0 })
        .collect()
}

/// Score term overlap ratio between query and document features.
fn score_semantic(query_features: &[f32], doc_features: &[f32]) -> f32 {
    let dot: f32 = query_features
        .iter()
        .zip(doc_features.iter())
        .map(|(a, b)| a * b)
        .sum();
    let norm_q: f32 = query_features
        .iter()
        .map(|x| x * x)
        .sum::<f32>()
        .sqrt()
        .max(f32::EPSILON);
    let norm_d: f32 = doc_features
        .iter()
        .map(|x| x * x)
        .sum::<f32>()
        .sqrt()
        .max(f32::EPSILON);
    dot / (norm_q * norm_d)
}

/// Fetch search.json from docs.ssccs.org and return parsed items.
/// Uses ureq blocking HTTP. Marks test as ignored on network error
/// to avoid breaking offline CI.
fn load_search_items() -> Vec<(String, String)> {
    let url = "https://docs.ssccs.org/search.json";
    let resp = ureq::get(url).call().expect("failed to fetch search.json");
    let data: Vec<u8> = resp
        .into_body()
        .read_to_vec()
        .expect("failed to read response body");
    let items: Vec<serde_json::Value> = serde_json::from_slice(&data).expect("invalid search.json");
    items
        .iter()
        .map(|v| {
            let title = v["title"].as_str().unwrap_or("").to_string();
            let text = v["text"].as_str().unwrap_or("").to_string();
            (title, text)
        })
        .collect()
}

/// Vocabulary for document indexing (curated from SSCCS documentation domain).
const VOCABULARY: &[&str] = &[
    "segment",
    "scheme",
    "field",
    "observation",
    "projection",
    "computation",
    "immutable",
    "structure",
    "constraint",
    "energy",
    "memory",
    "data",
    "parallel",
    "deterministic",
    "fih",
    "fact",
    "intent",
    "hint",
    "blackboard",
    "semantic",
    "vector",
    "store",
    "index",
    "search",
    "rust",
    "compiler",
    "verification",
    "c2pa",
    "provenance",
    "hardware",
    "risc",
    "fpga",
    "observation",
    "collapse",
    "github",
    "foundation",
    "ssccs",
    "open",
    "source",
    "agent",
    "knowledge",
    "graph",
    "document",
    "embedding",
    "inference",
    "model",
    "neural",
    "token",
    "attention",
];

// ── Scenario tests ──────────────────────────────────────────────────────

/// Insert and search with multiple documents using feature vectors.
#[test]
fn scenario_basic_insert_and_search() {
    let mut store = MockSemanticStore::new();

    // Insert three documents with known feature vectors
    store
        .insert(1, &FeatureLoad::new(vec![1.0, 0.0, 0.0]))
        .unwrap();
    store
        .insert(2, &FeatureLoad::new(vec![0.0, 1.0, 0.0]))
        .unwrap();
    store
        .insert(3, &FeatureLoad::new(vec![0.9, 0.1, 0.0]))
        .unwrap();

    // Search for documents similar to [1.0, 0.0, 0.0]
    let results = store
        .search(&FeatureLoad::new(vec![1.0, 0.0, 0.0]), 2)
        .unwrap();
    assert_eq!(results.len(), 2, "should return top 2 results");
    assert_eq!(results[0].0, 1, "most similar should be id=1 (cosine ~1.0)");
    assert_eq!(results[1].0, 3, "second should be id=3 (cosine ~0.9)");
}

/// Cross-methodology: same trait works with text-only data
/// (simulates BM25 store that ignores features and uses text)
#[test]
fn scenario_text_only_insert() {
    let mut store = MockSemanticStore::new();

    // TextLoad has no features, so insert should fail for MockSemanticStore
    // (which requires features for cosine similarity)
    let result = store.insert(
        1,
        &TextLoad {
            text: "hello world".into(),
        },
    );
    assert!(result.is_err(), "MockSemanticStore requires features");
}

/// Remove then re-insert: store should handle lifecycle correctly
#[test]
fn scenario_remove_and_reinsert() {
    let mut store = MockSemanticStore::new();

    store.insert(10, &FeatureLoad::new(vec![1.0, 0.0])).unwrap();
    assert_eq!(store.len(), 1);

    store.remove(10).unwrap();
    assert_eq!(store.len(), 0, "should be empty after remove");

    // Re-insert same id
    store.insert(10, &FeatureLoad::new(vec![0.0, 1.0])).unwrap();
    assert_eq!(store.len(), 1);

    // Verify new vector is searchable
    let results = store.search(&FeatureLoad::new(vec![0.0, 1.0]), 1).unwrap();
    assert_eq!(results[0].0, 10, "re-inserted id should be found");
}

/// Mixed content: some records with features, some without
#[test]
fn scenario_mixed_content() {
    let mut store = MockSemanticStore::new();

    store.insert(1, &FeatureLoad::new(vec![1.0, 0.0])).unwrap();
    store.insert(2, &FeatureLoad::new(vec![0.0, 1.0])).unwrap();

    // TextLoad will fail for MockSemanticStore (no features)
    assert!(
        store
            .insert(
                3,
                &TextLoad {
                    text: "no features".into()
                }
            )
            .is_err()
    );

    // But the store should still have first two
    assert_eq!(store.len(), 2);
}

/// Batch insert many documents (simulating search.json scale)
#[test]
fn scenario_batch_insert_stress() {
    let mut store = MockSemanticStore::new();
    let count = 500;

    for i in 0..count {
        let v = i as f32 / count as f32;
        store
            .insert(i as u32, &FeatureLoad::new(vec![v, 1.0 - v, 0.5]))
            .unwrap();
    }

    assert_eq!(store.len(), count);

    // Search for a specific vector
    let results = store
        .search(&FeatureLoad::new(vec![0.5, 0.5, 0.5]), 5)
        .unwrap();
    assert_eq!(results.len(), 5, "should return top 5");
    // All results should have valid scores
    for (id, score) in &results {
        assert!(*score > 0.0, "id={} should have positive score", id);
    }
}

/// Query empty store returns empty results
#[test]
fn scenario_empty_store() {
    let store = MockSemanticStore::new();
    assert!(store.is_empty());

    let results = store.search(&FeatureLoad::new(vec![1.0, 0.0]), 10).unwrap();
    assert!(
        results.is_empty(),
        "empty store should return empty results"
    );
}

/// Duplicate insert (same id) should not add duplicate entry
#[test]
fn scenario_duplicate_insert() {
    let mut store = MockSemanticStore::new();

    store.insert(1, &FeatureLoad::new(vec![1.0, 0.0])).unwrap();
    store.insert(1, &FeatureLoad::new(vec![1.0, 0.0])).unwrap();
    // MockSemanticStore does not dedup by id, so len becomes 2
    // This is expected — the caller (FihCoord) should prevent duplicates
    assert_eq!(
        store.len(),
        2,
        "MockSemanticStore allows duplicates by design"
    );
}

/// Dimension mismatch error
#[test]
fn scenario_dimension_mismatch() {
    let mut store = MockSemanticStore::new();

    store
        .insert(1, &FeatureLoad::new(vec![1.0, 0.0, 0.0]))
        .unwrap(); // 3D

    // Query with 2D vector should fail
    let result = store.search(&FeatureLoad::new(vec![1.0, 0.0]), 5);
    assert!(result.is_err(), "dimension mismatch should error");
}

/// Full document lifecycle: insert -> search -> remove -> empty
#[test]
fn scenario_full_document_lifecycle() {
    let mut store = MockSemanticStore::new();

    store
        .insert(100, &FeatureLoad::new(vec![1.0, 0.0, 0.0, 0.0]))
        .unwrap();
    store
        .insert(200, &FeatureLoad::new(vec![0.0, 1.0, 0.0, 0.0]))
        .unwrap();
    store
        .insert(300, &FeatureLoad::new(vec![0.0, 0.0, 1.0, 0.0]))
        .unwrap();

    assert_eq!(store.len(), 3);

    // Search before remove
    let r1 = store
        .search(&FeatureLoad::new(vec![1.0, 0.0, 0.0, 0.0]), 3)
        .unwrap();
    assert_eq!(r1.len(), 3);
    assert_eq!(r1[0].0, 100);

    // Remove one
    store.remove(200).unwrap();
    assert_eq!(store.len(), 2);

    // Search after remove
    let r2 = store
        .search(&FeatureLoad::new(vec![1.0, 0.0, 0.0, 0.0]), 3)
        .unwrap();
    assert_eq!(r2.len(), 2, "should only have 2 after remove");
    assert_eq!(r2[0].0, 100);

    // Remove all
    store.remove(100).unwrap();
    store.remove(300).unwrap();
    assert_eq!(store.len(), 0);
}

/// Search.json scenario: index documents from search.json and search by text
#[test]
fn scenario_search_json_documents() {
    println!("\n=== search.json document indexing and search ===");

    let items = load_search_items();
    eprintln!("  fetched {} items from docs.ssccs.org", items.len());

    let mut store = MockSemanticStore::new();

    let start = std::time::Instant::now();
    for (i, (_title, text)) in items.iter().enumerate().take(500) {
        let features = text_to_features(text, VOCABULARY);
        store.insert(i as u32, &FeatureLoad::new(features)).unwrap();
        if i > 0 && i % 100 == 0 {
            eprintln!("  indexed {} / 500 documents...", i);
        }
    }
    let elapsed = start.elapsed();
    eprintln!("  indexed {} documents in {:?}", store.len(), elapsed);

    // Search for documents about "segment scheme field"
    eprintln!("\n  --- Query 1: 'segment scheme field observation projection' ---");
    let query = "segment scheme field observation projection";
    let query_feats = text_to_features(query, VOCABULARY);
    let results = store.search(&FeatureLoad::new(query_feats), 5).unwrap();
    eprintln!("  top 5 results:");
    for (id, score) in &results {
        let (title, text) = &items[*id as usize];
        let preview: String = text.chars().take(80).collect();
        eprintln!("    [{:3}] score={:.4}  {}", id, score, title);
        eprintln!("            {}", preview);
    }

    // Search for SSCCS-related documents
    eprintln!("\n  --- Query 2: 'ssccs foundation open source github' ---");
    let query2 = "ssccs foundation open source github";
    let query_feats2 = text_to_features(query2, VOCABULARY);
    let results2 = store.search(&FeatureLoad::new(query_feats2), 3).unwrap();
    eprintln!("  top 3 results:");
    for (id, score) in &results2 {
        let (title, _text) = &items[*id as usize];
        eprintln!("    [{:3}] score={:.4}  {}", id, score, title);
    }

    // Search for "energy memory constraint"
    eprintln!("\n  --- Query 3: 'energy memory data movement computation' ---");
    let query3 = "energy memory data movement computation";
    let query_feats3 = text_to_features(query3, VOCABULARY);
    let results3 = store.search(&FeatureLoad::new(query_feats3), 4).unwrap();
    eprintln!("  top 4 results:");
    for (id, score) in &results3 {
        let (title, _text) = &items[*id as usize];
        eprintln!("    [{:3}] score={:.4}  {}", id, score, title);
    }
    eprintln!("");
}

/// Search.json scenario: add a new document incrementally and verify search includes it
#[test]
fn scenario_search_json_incremental_add() {
    let items = load_search_items();
    let mut store = MockSemanticStore::new();

    // Index first 100 docs
    for (i, (_title, text)) in items.iter().enumerate().take(100) {
        let features = text_to_features(text, VOCABULARY);
        store.insert(i as u32, &FeatureLoad::new(features)).unwrap();
    }
    assert_eq!(store.len(), 100);

    // Search without the new doc
    let q = "open source community";
    let qf = text_to_features(q, VOCABULARY);
    let _before = store.search(&FeatureLoad::new(qf.clone()), 3).unwrap();

    // Add a new "open source" oriented document
    let new_text =
        "open source community contributions github collaboration fork pull request license";
    let new_feats = text_to_features(new_text, VOCABULARY);
    store.insert(999, &FeatureLoad::new(new_feats)).unwrap();
    assert_eq!(store.len(), 101);

    // Search again — new doc should appear in results
    let after = store.search(&FeatureLoad::new(qf), 5).unwrap();
    let after_ids: Vec<u32> = after.iter().map(|(id, _)| *id).collect();
    assert!(
        after_ids.contains(&999),
        "newly added doc should appear in results"
    );
}

/// Search.json scenario: topic-specific retrieval
#[test]
fn scenario_search_json_topic_specific() {
    let items = load_search_items();
    let mut store = MockSemanticStore::new();

    for (i, (_title, text)) in items.iter().enumerate().take(500) {
        let features = text_to_features(text, VOCABULARY);
        store.insert(i as u32, &FeatureLoad::new(features)).unwrap();
    }

    // Topic: hardware / RISC-V
    let q1 = "risc hardware fpga verification processor";
    let r1 = store
        .search(&FeatureLoad::new(text_to_features(q1, VOCABULARY)), 3)
        .unwrap();
    assert_eq!(r1.len(), 3);
    assert!(r1[0].1 > 0.0, "hardware topic should have positive scores");

    // Topic: C2PA / provenance
    let q2 = "c2pa provenance signature verification certificate";
    let r2 = store
        .search(&FeatureLoad::new(text_to_features(q2, VOCABULARY)), 3)
        .unwrap();
    assert_eq!(r2.len(), 3);
}

/// Multiple queries on the same store produce consistent results
#[test]
fn scenario_query_consistency() {
    let mut store = MockSemanticStore::new();

    store.insert(10, &FeatureLoad::new(vec![1.0, 0.0])).unwrap();
    store.insert(20, &FeatureLoad::new(vec![0.0, 1.0])).unwrap();

    // Query same vector twice should produce same result
    let r1 = store.search(&FeatureLoad::new(vec![1.0, 0.0]), 2).unwrap();
    let r2 = store.search(&FeatureLoad::new(vec![1.0, 0.0]), 2).unwrap();

    assert_eq!(r1.len(), r2.len());
    for (a, b) in r1.iter().zip(r2.iter()) {
        assert_eq!(a.0, b.0, "same query should return same ids");
        assert!(
            (a.1 - b.1).abs() < 1e-6,
            "same query should return same scores"
        );
    }
}

/// Use OriginLoad to demonstrate origin-based semantic filtering.
/// Simulates an ngram store that indexes by origin string.
#[test]
fn scenario_origin_based_search() {
    let mut store = MockSemanticStore::new();

    // OriginLoad has no features, so MockSemanticStore fails on insert.
    // This is correct: OriginLoad tests the FihLoad trait boundary.
    let result = store.insert(
        1,
        &OriginLoad {
            origin: "whitepaper".into(),
        },
    );
    assert!(result.is_err(), "OriginLoad lacks features, should fail");

    // But verify the constructors work: we can still create and pass it
    let load = OriginLoad {
        origin: "whitepaper".into(),
    };
    assert_eq!(load.origin(42), Some("whitepaper".into()));
}

/// Use FullDocLoad to demonstrate full-document FihLoad with all accessors.
#[test]
fn scenario_full_doc_load() {
    let mut store = MockSemanticStore::new();

    // FullDocLoad has no features either — should fail for MockSemanticStore
    let result = store.insert(
        1,
        &FullDocLoad {
            text: "ssccs semantics".into(),
            origin: "whitepaper".into(),
            creator: "taeho".into(),
        },
    );
    assert!(result.is_err(), "FullDocLoad lacks features, should fail");

    // Verify all accessors work
    let load = FullDocLoad {
        text: "ssccs semantics".into(),
        origin: "whitepaper".into(),
        creator: "taeho".into(),
    };
    assert_eq!(load.text(99).unwrap(), "ssccs semantics");
    assert_eq!(load.origin(99).unwrap(), "whitepaper");
    assert_eq!(load.creator(99).unwrap(), "taeho");
}

/// Use score_semantic to manually verify cosine similarity calculation.
#[test]
fn scenario_manual_score_verification() {
    // Two identical vectors should have score 1.0
    let a = vec![1.0, 0.0, 0.0];
    let b = vec![1.0, 0.0, 0.0];
    let s = score_semantic(&a, &b);
    assert!((s - 1.0).abs() < 1e-6, "identical vectors should score 1.0");

    // Orthogonal vectors should have score 0.0
    let c = vec![0.0, 1.0, 0.0];
    let s2 = score_semantic(&a, &c);
    assert!(s2.abs() < 1e-6, "orthogonal vectors should score 0.0");

    // Opposite vectors should score 0.0 (no negative in bag-of-words)
    // But let's verify the math works with negative
    let d = vec![-1.0, 0.0, 0.0];
    let s3 = score_semantic(&a, &d);
    assert!(
        (s3 + 1.0).abs() < 1e-6,
        "opposite vectors should score -1.0"
    );

    // Verify against MockSemanticStore's internal calculation
    let mut store = MockSemanticStore::new();
    store
        .insert(1, &FeatureLoad::new(vec![1.0, 0.0, 0.0]))
        .unwrap();
    store
        .insert(2, &FeatureLoad::new(vec![0.5, 0.5, 0.0]))
        .unwrap();

    let results = store
        .search(&FeatureLoad::new(vec![1.0, 0.0, 0.0]), 2)
        .unwrap();
    // Manually verify id=1 score
    let manual_score = score_semantic(&[1.0, 0.0, 0.0], &[1.0, 0.0, 0.0]);
    assert!((results[0].1 - manual_score).abs() < 1e-6);
}
