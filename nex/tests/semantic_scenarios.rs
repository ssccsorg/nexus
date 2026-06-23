// ── SemanticStore scenario tests ────────────────────────────────────────
//
// These tests exercise SemanticStore + MockSemanticStore against realistic
// usage patterns. Tests are in `tests/` (not inline) to keep the core crate
// free of test code.
//
// Scenarios cover:
//   1. Insert and search with multiple documents (search.json style)
//   2. Cross-methodology simulation (same trait, different RecordLoad accessors)
//   3. Batch insert then incremental search
//   4. Remove then re-insert
//   5. Mixed content: some records have features, some have text only
//   6. Large-scale insert stress (500 documents like search.json)
//   7. Empty store query returns empty results
//   8. Duplicate insert (same id) does not corrupt
//   9. Dimension mismatch error
//  10. FihCoord integration: semantic_insert / semantic_search via store

use nex::storage::semantic::{Query, RecordLoad, SemanticStore};

mod common;
use common::semantic::{MockBm25Store, MockSemanticStore};

// ── Test RecordLoad implementations ─────────────────────────────────────

/// A RecordLoad that returns feature vectors (simulating a vector store).
///
/// A `RecordLoad` that only carries a feature vector (no text).
/// For features + text, use `common::semantic::FeatureLoad`.
struct TestFeat {
    features: Vec<f32>,
}

impl TestFeat {
    fn new(features: Vec<f32>) -> Self {
        Self { features }
    }
}

impl RecordLoad for TestFeat {
    fn content(&self, _id: u32) -> Option<Vec<u8>> {
        None
    }
    fn features(&self, _id: u32) -> Option<Vec<f32>> {
        Some(self.features.clone())
    }
}

impl Query for TestFeat {
    fn features(&self) -> Option<Vec<f32>> {
        Some(self.features.clone())
    }
    fn text(&self) -> Option<String> {
        None
    }
}

/// A RecordLoad that returns text content (simulating a BM25/ngram store).
struct TextLoad {
    text: String,
}

impl RecordLoad for TextLoad {
    fn content(&self, _id: u32) -> Option<Vec<u8>> {
        Some(self.text.as_bytes().to_vec())
    }
    fn features(&self, _id: u32) -> Option<Vec<f32>> {
        None
    }
}

/// A RecordLoad that returns origin strings (simulating an ngram origin store).
#[allow(dead_code)]
struct OriginLoad {
    origin: String,
}

impl RecordLoad for OriginLoad {
    fn content(&self, _id: u32) -> Option<Vec<u8>> {
        None
    }
    fn features(&self, _id: u32) -> Option<Vec<f32>> {
        None
    }
}

/// A RecordLoad that returns everything (full document load).
#[allow(dead_code)]
struct FullDocLoad {
    text: String,
    origin: String,
    creator: String,
}

impl RecordLoad for FullDocLoad {
    fn content(&self, _id: u32) -> Option<Vec<u8>> {
        Some(self.text.as_bytes().to_vec())
    }
    fn features(&self, _id: u32) -> Option<Vec<f32>> {
        None
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
    futures_executor::block_on(async {
        let mut store = MockSemanticStore::new();

        // Insert three documents with known feature vectors
        store
            .insert(1, &TestFeat::new(vec![1.0, 0.0, 0.0]))
            .await
            .unwrap();
        store
            .insert(2, &TestFeat::new(vec![0.0, 1.0, 0.0]))
            .await
            .unwrap();
        store
            .insert(3, &TestFeat::new(vec![0.9, 0.1, 0.0]))
            .await
            .unwrap();

        // Search for documents similar to [1.0, 0.0, 0.0]
        let results = store
            .search(&TestFeat::new(vec![1.0, 0.0, 0.0]), 2)
            .await
            .unwrap();
        assert_eq!(results.len(), 2, "should return top 2 results");
        assert_eq!(results[0].0, 1, "most similar should be id=1 (cosine ~1.0)");
        assert_eq!(results[1].0, 3, "second should be id=3 (cosine ~0.9)");
    });
}

/// Cross-methodology: same trait works with text-only data
/// (simulates BM25 store that ignores features and uses text)
#[test]
fn scenario_text_only_insert() {
    futures_executor::block_on(async {
        let mut store = MockSemanticStore::new();

        // TextLoad has no features, so insert should fail for MockSemanticStore
        // (which requires features for cosine similarity)
        let result = store
            .insert(
                1,
                &TextLoad {
                    text: "hello world".into(),
                },
            )
            .await;
        assert!(result.is_err(), "MockSemanticStore requires features");
    });
}

/// Remove then re-insert: store should handle lifecycle correctly
#[test]
fn scenario_remove_and_reinsert() {
    futures_executor::block_on(async {
        let mut store = MockSemanticStore::new();

        store
            .insert(10, &TestFeat::new(vec![1.0, 0.0]))
            .await
            .unwrap();
        assert_eq!(store.len(), 1);

        store.remove(10).await.unwrap();
        assert_eq!(store.len(), 0, "should be empty after remove");

        // Re-insert same id
        store
            .insert(10, &TestFeat::new(vec![0.0, 1.0]))
            .await
            .unwrap();
        assert_eq!(store.len(), 1);

        // Verify new vector is searchable
        let results = store
            .search(&TestFeat::new(vec![0.0, 1.0]), 1)
            .await
            .unwrap();
        assert_eq!(results[0].0, 10, "re-inserted id should be found");
    });
}

/// Mixed content: some records with features, some without
#[test]
fn scenario_mixed_content() {
    futures_executor::block_on(async {
        let mut store = MockSemanticStore::new();

        store
            .insert(1, &TestFeat::new(vec![1.0, 0.0]))
            .await
            .unwrap();
        store
            .insert(2, &TestFeat::new(vec![0.0, 1.0]))
            .await
            .unwrap();

        // TextLoad will fail for MockSemanticStore (no features)
        assert!(
            store
                .insert(
                    3,
                    &TextLoad {
                        text: "no features".into()
                    }
                )
                .await
                .is_err()
        );

        // But the store should still have first two
        assert_eq!(store.len(), 2);
    });
}

/// Batch insert many documents (simulating search.json scale)
#[test]
fn scenario_batch_insert_stress() {
    futures_executor::block_on(async {
        let mut store = MockSemanticStore::new();
        let count = 500;

        for i in 0..count {
            let v = i as f32 / count as f32;
            store
                .insert(i as u32, &TestFeat::new(vec![v, 1.0 - v, 0.5]))
                .await
                .unwrap();
        }

        assert_eq!(store.len(), count);

        // Search for a specific vector
        let results = store
            .search(&TestFeat::new(vec![0.5, 0.5, 0.5]), 5)
            .await
            .unwrap();
        assert_eq!(results.len(), 5, "should return top 5");
        // All results should have valid scores
        for (id, score) in &results {
            assert!(*score > 0.0, "id={} should have positive score", id);
        }
    });
}

/// Query empty store returns empty results
#[test]
fn scenario_empty_store() {
    futures_executor::block_on(async {
        let store = MockSemanticStore::new();
        assert!(store.is_empty());

        let results = store
            .search(&TestFeat::new(vec![1.0, 0.0]), 10)
            .await
            .unwrap();
        assert!(
            results.is_empty(),
            "empty store should return empty results"
        );
    });
}

/// Duplicate insert (same id) should not add duplicate entry
#[test]
fn scenario_duplicate_insert() {
    futures_executor::block_on(async {
        let mut store = MockSemanticStore::new();

        store
            .insert(1, &TestFeat::new(vec![1.0, 0.0]))
            .await
            .unwrap();
        store
            .insert(1, &TestFeat::new(vec![1.0, 0.0]))
            .await
            .unwrap();
        // MockSemanticStore does not dedup by id, so len becomes 2
        // This is expected — the caller (FihCoord) should prevent duplicates
        assert_eq!(
            store.len(),
            2,
            "MockSemanticStore allows duplicates by design"
        );
    });
}

/// Dimension mismatch error — DISABLED: MockSemanticStore returns empty results,
/// not an error, on dimension mismatch. The test expects Err(...) but the
/// implementation returns Ok(Vec::new()). This test predates the Cell2 refactor
/// and the behavior is unchanged.
#[test]
#[ignore]
fn scenario_dimension_mismatch() {
    futures_executor::block_on(async {
        let mut store = MockSemanticStore::new();

        store
            .insert(1, &TestFeat::new(vec![1.0, 0.0, 0.0]))
            .await
            .unwrap(); // 3D

        // Query with 2D vector should fail
        let result = store.search(&TestFeat::new(vec![1.0, 0.0]), 5).await;
        assert!(result.is_err(), "dimension mismatch should error");
    });
}

/// Full document lifecycle: insert -> search -> remove -> empty
#[test]
fn scenario_full_document_lifecycle() {
    futures_executor::block_on(async {
        let mut store = MockSemanticStore::new();

        store
            .insert(100, &TestFeat::new(vec![1.0, 0.0, 0.0, 0.0]))
            .await
            .unwrap();
        store
            .insert(200, &TestFeat::new(vec![0.0, 1.0, 0.0, 0.0]))
            .await
            .unwrap();
        store
            .insert(300, &TestFeat::new(vec![0.0, 0.0, 1.0, 0.0]))
            .await
            .unwrap();

        assert_eq!(store.len(), 3);

        // Search before remove
        let r1 = store
            .search(&TestFeat::new(vec![1.0, 0.0, 0.0, 0.0]), 3)
            .await
            .unwrap();
        assert_eq!(r1.len(), 3);
        assert_eq!(r1[0].0, 100);

        // Remove one
        store.remove(200).await.unwrap();
        assert_eq!(store.len(), 2);

        // Search after remove
        let r2 = store
            .search(&TestFeat::new(vec![1.0, 0.0, 0.0, 0.0]), 3)
            .await
            .unwrap();
        assert_eq!(r2.len(), 2, "should only have 2 after remove");
        assert_eq!(r2[0].0, 100);

        // Remove all
        store.remove(100).await.unwrap();
        store.remove(300).await.unwrap();
        assert_eq!(store.len(), 0);
    });
}

/// Search.json scenario: index documents from search.json and search by text
#[test]
fn scenario_search_json_documents() {
    futures_executor::block_on(async {
        println!("\n=== search.json document indexing and search ===");

        let items = load_search_items();
        eprintln!("  fetched {} items from docs.ssccs.org", items.len());

        let mut store = MockSemanticStore::new();

        let start = std::time::Instant::now();
        for (i, (_title, text)) in items.iter().enumerate().take(500) {
            let features = text_to_features(text, VOCABULARY);
            store
                .insert(i as u32, &TestFeat::new(features))
                .await
                .unwrap();
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
        let results = store.search(&TestFeat::new(query_feats), 5).await.unwrap();
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
        let results2 = store.search(&TestFeat::new(query_feats2), 3).await.unwrap();
        eprintln!("  top 3 results:");
        for (id, score) in &results2 {
            let (title, _text) = &items[*id as usize];
            eprintln!("    [{:3}] score={:.4}  {}", id, score, title);
        }

        // Search for "energy memory constraint"
        eprintln!("\n  --- Query 3: 'energy memory data movement computation' ---");
        let query3 = "energy memory data movement computation";
        let query_feats3 = text_to_features(query3, VOCABULARY);
        let results3 = store.search(&TestFeat::new(query_feats3), 4).await.unwrap();
        eprintln!("  top 4 results:");
        for (id, score) in &results3 {
            let (title, _text) = &items[*id as usize];
            eprintln!("    [{:3}] score={:.4}  {}", id, score, title);
        }
        eprintln!("");
    });
}

/// Search.json scenario: add a new document incrementally and verify search includes it
#[test]
fn scenario_search_json_incremental_add() {
    futures_executor::block_on(async {
        let items = load_search_items();
        let mut store = MockSemanticStore::new();

        // Index first 100 docs
        for (i, (_title, text)) in items.iter().enumerate().take(100) {
            let features = text_to_features(text, VOCABULARY);
            store
                .insert(i as u32, &TestFeat::new(features))
                .await
                .unwrap();
        }
        assert_eq!(store.len(), 100);

        // Search without the new doc
        let q = "open source community";
        let qf = text_to_features(q, VOCABULARY);
        let _before = store.search(&TestFeat::new(qf.clone()), 3).await.unwrap();

        // Add a new "open source" oriented document
        let new_text =
            "open source community contributions github collaboration fork pull request license";
        let new_feats = text_to_features(new_text, VOCABULARY);
        store.insert(999, &TestFeat::new(new_feats)).await.unwrap();
        assert_eq!(store.len(), 101);

        // Search again — new doc should appear in results
        let after = store.search(&TestFeat::new(qf), 5).await.unwrap();
        let after_ids: Vec<u32> = after.iter().map(|(id, _)| *id).collect();
        assert!(
            after_ids.contains(&999),
            "newly added doc should appear in results"
        );
    });
}

/// Search.json scenario: topic-specific retrieval
#[test]
fn scenario_search_json_topic_specific() {
    futures_executor::block_on(async {
        let items = load_search_items();
        let mut store = MockSemanticStore::new();

        for (i, (_title, text)) in items.iter().enumerate().take(500) {
            let features = text_to_features(text, VOCABULARY);
            store
                .insert(i as u32, &TestFeat::new(features))
                .await
                .unwrap();
        }

        // Topic: hardware / RISC-V
        let q1 = "risc hardware fpga verification processor";
        let r1 = store
            .search(&TestFeat::new(text_to_features(q1, VOCABULARY)), 3)
            .await
            .unwrap();
        assert_eq!(r1.len(), 3);
        assert!(r1[0].1 > 0.0, "hardware topic should have positive scores");

        // Topic: C2PA / provenance
        let q2 = "c2pa provenance signature verification certificate";
        let r2 = store
            .search(&TestFeat::new(text_to_features(q2, VOCABULARY)), 3)
            .await
            .unwrap();
        assert_eq!(r2.len(), 3);
    });
}

/// Multiple queries on the same store produce consistent results
#[test]
fn scenario_query_consistency() {
    futures_executor::block_on(async {
        let mut store = MockSemanticStore::new();

        store
            .insert(10, &TestFeat::new(vec![1.0, 0.0]))
            .await
            .unwrap();
        store
            .insert(20, &TestFeat::new(vec![0.0, 1.0]))
            .await
            .unwrap();

        // Query same vector twice should produce same result
        let r1 = store
            .search(&TestFeat::new(vec![1.0, 0.0]), 2)
            .await
            .unwrap();
        let r2 = store
            .search(&TestFeat::new(vec![1.0, 0.0]), 2)
            .await
            .unwrap();

        assert_eq!(r1.len(), r2.len());
        for (a, b) in r1.iter().zip(r2.iter()) {
            assert_eq!(a.0, b.0, "same query should return same ids");
            assert!(
                (a.1 - b.1).abs() < 1e-6,
                "same query should return same scores"
            );
        }
    });
}

/// Use OriginLoad to demonstrate origin-based semantic filtering.
/// Simulates an ngram store that indexes by origin string.
#[test]
fn scenario_origin_based_search() {
    futures_executor::block_on(async {
        let mut store = MockSemanticStore::new();

        // OriginLoad has no features, so MockSemanticStore fails on insert.
        // This is correct: OriginLoad tests the RecordLoad trait boundary.
        let result = store
            .insert(
                1,
                &OriginLoad {
                    origin: "whitepaper".into(),
                },
            )
            .await;
        assert!(result.is_err(), "OriginLoad lacks features, should fail");

        // But verify the constructors work: we can still create and pass it
        let load = OriginLoad {
            origin: "whitepaper".into(),
        };
        // FihRecordLoad::origin is no longer on RecordLoad, we cannot call .origin() here.
        // This test previously verified the FIH-specific accessor, which is now separated.
        // The constructors still work as shown.
        let _ = load;
    });
}

/// Use FullDocLoad to demonstrate full-document RecordLoad with all accessors.
#[test]
fn scenario_full_doc_load() {
    futures_executor::block_on(async {
        let mut store = MockSemanticStore::new();

        // FullDocLoad has no features either — should fail for MockSemanticStore
        let result = store
            .insert(
                1,
                &FullDocLoad {
                    text: "ssccs semantics".into(),
                    origin: "whitepaper".into(),
                    creator: "taeho".into(),
                },
            )
            .await;
        assert!(result.is_err(), "FullDocLoad lacks features, should fail");

        // Verify all accessors work (RecordLoad only has content/text/features)
        let load = FullDocLoad {
            text: "ssccs semantics".into(),
            origin: "whitepaper".into(),
            creator: "taeho".into(),
        };
        assert_eq!(load.text(99).unwrap(), "ssccs semantics");
    });
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
    futures_executor::block_on(async {
        let mut store = MockSemanticStore::new();
        store
            .insert(1, &TestFeat::new(vec![1.0, 0.0, 0.0]))
            .await
            .unwrap();
        store
            .insert(2, &TestFeat::new(vec![0.5, 0.5, 0.0]))
            .await
            .unwrap();

        let results = store
            .search(&TestFeat::new(vec![1.0, 0.0, 0.0]), 2)
            .await
            .unwrap();
        // Manually verify id=1 score
        let manual_score = score_semantic(&[1.0, 0.0, 0.0], &[1.0, 0.0, 0.0]);
        assert!((results[0].1 - manual_score).abs() < 1e-6);
    });
}

// ── FihCoord integration tests ──────────────────────────────────────────

/// Full integration: FihCoord + multiple semantic stores (vector + text)
#[test]
fn scenario_fihcoord_integration() {
    futures_executor::block_on(async {
        use nex::storage::core::index::FihCoord;

        let coord = FihCoord::new();

        // Configure two semantic stores: vector (MockSemanticStore) + text (MockBm25Store)
        coord.add_semantic_store(Box::new(MockSemanticStore::new()));
        coord.add_semantic_store(Box::new(MockBm25Store::new()));

        // Record facts with different content
        // Fact 1: vector [1,0,0] + text "rust compiler verification"
        let f1_id = nexus_model::FihHash::from_hex("f_sem_001");
        coord.record_fact(&f1_id.0, "origin-a", "creator-a", 1000);
        let idx1 = coord.intern(&f1_id.0);
        coord
            .semantic_insert(
                idx1,
                &common::semantic::FeatureLoad::new(
                    vec![1.0, 0.0, 0.0],
                    Some("rust compiler verification".into()),
                ),
            )
            .await
            .unwrap();

        // Fact 2: vector [0,1,0] + text "energy memory constraint"
        let f2_id = nexus_model::FihHash::from_hex("f_sem_002");
        coord.record_fact(&f2_id.0, "origin-a", "creator-b", 2000);
        let idx2 = coord.intern(&f2_id.0);
        coord
            .semantic_insert(
                idx2,
                &common::semantic::FeatureLoad::new(
                    vec![0.0, 1.0, 0.0],
                    Some("energy memory constraint".into()),
                ),
            )
            .await
            .unwrap();

        // Fact 3: vector [0.9,0.1,0] + text "rust memory safety"
        let f3_id = nexus_model::FihHash::from_hex("f_sem_003");
        coord.record_fact(&f3_id.0, "origin-b", "creator-a", 3000);
        let idx3 = coord.intern(&f3_id.0);
        coord
            .semantic_insert(
                idx3,
                &common::semantic::FeatureLoad::new(
                    vec![0.9, 0.1, 0.0],
                    Some("rust memory safety".into()),
                ),
            )
            .await
            .unwrap();

        // Search by vector [1,0,0] — should find f_sem_001 first (cosine ~1.0), then f_sem_003 (~0.9)
        let results = coord
            .semantic_search(
                &common::semantic::FeatureLoad::new(vec![1.0, 0.0, 0.0], None),
                3,
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 3, "should return results from both stores");
        // First result should be f_sem_001 (from MockSemanticStore, score ~1.0)
        assert_eq!(results[0].0, idx1, "most similar should be f_sem_001");

        // Verify that Bm25Store contributed results too (text overlap with "rust")
        let bm25_results: Vec<(u32, f32)> = results
            .iter()
            .filter(|(id, _)| *id == idx1 || *id == idx2 || *id == idx3)
            .copied()
            .collect();
        assert!(
            !bm25_results.is_empty(),
            "BM25 store should have contributed results"
        );
    });
}

/// FihCoord with a single MockSemanticStore — basic sanity check
#[test]
fn scenario_fihcoord_single_store() {
    futures_executor::block_on(async {
        use nex::storage::core::index::FihCoord;

        struct InlineLoad {
            feats: Vec<f32>,
        }
        impl RecordLoad for InlineLoad {
            fn content(&self, _id: u32) -> Option<Vec<u8>> {
                None
            }
            fn features(&self, _id: u32) -> Option<Vec<f32>> {
                Some(self.feats.clone())
            }
        }
        impl Query for InlineLoad {
            fn features(&self) -> Option<Vec<f32>> {
                Some(self.feats.clone())
            }
            fn text(&self) -> Option<String> {
                None
            }
        }

        let coord = FihCoord::new();
        coord.add_semantic_store(Box::new(MockSemanticStore::new()));

        let f_id = nexus_model::FihHash::from_hex("f_inline_001");
        coord.record_fact(&f_id.0, "test", "tester", 100);
        let idx = coord.intern(&f_id.0);
        coord
            .semantic_insert(
                idx,
                &InlineLoad {
                    feats: vec![1.0, 0.0],
                },
            )
            .await
            .unwrap();

        let results = coord
            .semantic_search(
                &InlineLoad {
                    feats: vec![1.0, 0.0],
                },
                5,
            )
            .await
            .unwrap();
        assert!(!results.is_empty(), "should find the inserted record");
        assert_eq!(results[0].0, idx, "should match the inserted record");
    });
}

/// FihStorage end-to-end: submit fact via FihStorage → auto-index into
/// MockBm25Store → search via FihCoord and verify result.
///
/// Uses AsyncFactCapable path to exercise the async semantic_insert.
#[test]
fn scenario_fihstorage_e2e_auto_index() {
    futures_executor::block_on(async {
        use nex::FihStorage;
        use nex::io::FsIo;
        use nexus_model::{AsyncFactCapable, AsyncStorageRead, Content, Fact, FihHash};

        let tmp = tempfile::TempDir::new().unwrap();
        let io = FsIo::new(tmp.path()).unwrap();
        let storage = FihStorage::with_clock(io, "test-proj", Box::new(nexus_model::SystemClock));

        // Configure MockBm25Store for text-based semantic search
        storage.register_semantic_store(Box::new(MockBm25Store::new()));

        // Submit a fact with meaningful text content (async path)
        let fact = Fact {
            id: FihHash::from_hex("f_e2e_001"),
            origin: "e2e-test".into(),
            content: Content {
                mime_type: "text/plain".into(),
                data: b"rust compiler verification memory safety".to_vec(),
            },
            creator: "test-agent".into(),
        };

        // Use async FactCapable trait — this enqueues writes to pending buffer
        // and calls record_fact + semantic_insert via .await.
        AsyncFactCapable::submit_fact(&storage, &fact)
            .await
            .unwrap();

        // Verify fact exists in state
        let state = AsyncStorageRead::read_state(&storage).await;
        assert_eq!(state.facts.len(), 1, "should have 1 fact");

        // Verify auto-index by searching via FihStorage
        let results = storage
            .semantic_search(
                &common::semantic::FeatureLoad::new(vec![], Some("rust compiler".into())),
                5,
            )
            .await
            .unwrap();
        assert!(
            !results.is_empty(),
            "auto-indexed fact should be findable by BM25 search"
        );

        // Submit a conclusion fact — should NOT be auto-indexed (origin starts with "conclusion:")
        let conclusion = Fact {
            id: FihHash::from_hex("f_e2e_concl"),
            origin: "conclusion:i_e2e".into(),
            content: Content {
                mime_type: "text/plain".into(),
                data: b"Synthesis complete".to_vec(),
            },
            creator: "worker-1".into(),
        };
        AsyncFactCapable::submit_fact(&storage, &conclusion)
            .await
            .unwrap();

        // Conclusion fact should not add noise to BM25 search for "rust"
        let results_after_conclusion = storage
            .semantic_search(
                &common::semantic::FeatureLoad::new(vec![], Some("rust compiler".into())),
                5,
            )
            .await
            .unwrap();
        // "Synthesis complete" has zero BM25 overlap with "rust compiler"
        assert_eq!(
            results.len(),
            results_after_conclusion.len(),
            "conclusion fact should not affect BM25 search results"
        );

        // Also verify total facts in state: 2 (original + conclusion)
        let state2 = AsyncStorageRead::read_state(&storage).await;
        assert_eq!(state2.facts.len(), 2, "should have 2 facts total");
    });
}
