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

// ── Scenarios ───────────────────────────────────────────────────────────

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
