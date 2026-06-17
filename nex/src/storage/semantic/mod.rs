// ── SemanticStore: semantic feature store for similarity search ────────
//
// Another kind of Store, like EntityStore for identity-based access.
// SemanticStore associates record IDs with semantic features for
// similarity-based retrieval.
//
// The trait is designed as a thin "flashlight" interface: the core (FihCoord)
// provides only a lookup handle (`FihLoad`), and each external implementation
// decides which data it needs (text, embeddings, graph relations, etc.)
// by requesting it through the handle. This keeps the core agnostic to
// any specific methodology — vector, BM25, ngram, hypergraph, LLM reranker.
//
// Implementations (plug-in via USB hub pattern, living outside nex core):
//   - storage/semantic/vector/  — HNSW-based ANN vector search
//   - storage/semantic/bm25/    — BM25 string similarity (no LLM)
//   - storage/semantic/ngram/   — ngram-based fuzzy matching
//   - MockSemanticStore         — in-memory brute force (testing, in this file)
//
// nex core defines only the trait. External crates provide impls.

use std::fmt::Debug;

/// Lookup handle provided by FihCoord for retrieving Fih data.
///
/// Each `SemanticStore` implementation uses this handle to load only the
/// data it actually needs (text, feature vectors, content bytes, etc.).
pub trait FihLoad {
    /// Load content bytes for a record by its coord index.
    fn content(&self, id: u32) -> Option<Vec<u8>>;

    /// Load content decoded as UTF-8 text.
    fn text(&self, id: u32) -> Option<String> {
        self.content(id)
            .and_then(|bytes| String::from_utf8(bytes).ok())
    }

    /// Load f32 feature vector, if the record has one stored.
    fn features(&self, id: u32) -> Option<Vec<f32>>;

    /// Load the origin string for a fact record.
    fn origin(&self, id: u32) -> Option<String>;

    /// Load the creator string for a fact or intent record.
    fn creator(&self, id: u32) -> Option<String>;
}

/// Semantic feature store for similarity search.
///
/// Maps semantic features to record IDs. Used by FihCoord as another
/// index axis alongside by_origin, by_creator, by_status, etc.
///
/// Each implementation decides which data to extract via `FihLoad`.
pub trait SemanticStore: Debug {
    /// Insert a record into the store. The implementation calls
    /// `load` to retrieve only the data it needs.
    fn insert(&mut self, id: u32, load: &dyn FihLoad) -> Result<(), String>;

    /// Search for the top_k most similar records. The implementation
    /// may use `load` to refine search parameters or fetch comparison data.
    fn search(&self, query: &dyn FihLoad, top_k: usize) -> Result<Vec<(u32, f32)>, String>;

    /// Remove a record ID from the store.
    fn remove(&mut self, id: u32) -> Result<(), String>;

    /// Number of records stored.
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ── Mock in-memory SemanticStore ────────────────────────────────────────

/// Mock in-memory SemanticStore for testing.
///
/// Uses brute-force cosine similarity on feature vectors obtained
/// via `FihLoad::features`. Demonstrates the flashlight pattern:
/// the store never owns or knows about Fih internals.
#[derive(Debug)]
pub struct MockSemanticStore {
    ids: Vec<u32>,
    vectors: Vec<Vec<f32>>,
}

impl MockSemanticStore {
    pub fn new() -> Self {
        Self {
            ids: Vec::new(),
            vectors: Vec::new(),
        }
    }
}

impl SemanticStore for MockSemanticStore {
    fn insert(&mut self, id: u32, load: &dyn FihLoad) -> Result<(), String> {
        let features = load
            .features(id)
            .ok_or_else(|| "no features available".to_string())?;
        self.ids.push(id);
        self.vectors.push(features);
        Ok(())
    }

    fn search(&self, query: &dyn FihLoad, top_k: usize) -> Result<Vec<(u32, f32)>, String> {
        let query_vec = query
            .features(0)
            .ok_or_else(|| "no query features".to_string())?;
        if query_vec.len()
            != self
                .vectors
                .first()
                .map(|v| v.len())
                .unwrap_or(query_vec.len())
        {
            return Err("dimension mismatch".into());
        }
        let mut scores: Vec<(u32, f32)> = self
            .ids
            .iter()
            .zip(self.vectors.iter())
            .map(|(&id, vec)| {
                let dot: f32 = query_vec.iter().zip(vec.iter()).map(|(a, b)| a * b).sum();
                let norm_q: f32 = query_vec.iter().map(|x| x * x).sum::<f32>().sqrt();
                let norm_v: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
                let similarity = dot / (norm_q * norm_v).max(f32::EPSILON);
                (id, similarity)
            })
            .collect();
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
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

    fn len(&self) -> usize {
        self.ids.len()
    }
}

impl Default for MockSemanticStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestLoad {
        features: Vec<f32>,
    }
    impl FihLoad for TestLoad {
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

    #[test]
    fn test_mock_insert_and_search() {
        let mut store = MockSemanticStore::new();

        store
            .insert(
                10,
                &TestLoad {
                    features: vec![1.0, 0.0],
                },
            )
            .unwrap();
        store
            .insert(
                20,
                &TestLoad {
                    features: vec![0.0, 1.0],
                },
            )
            .unwrap();
        store
            .insert(
                30,
                &TestLoad {
                    features: vec![0.9, 0.1],
                },
            )
            .unwrap();

        let results = store
            .search(
                &TestLoad {
                    features: vec![1.0, 0.0],
                },
                2,
            )
            .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, 10); // most similar
        assert_eq!(results[1].0, 30); // second
    }

    #[test]
    fn test_mock_remove() {
        let mut store = MockSemanticStore::new();
        store
            .insert(
                10,
                &TestLoad {
                    features: vec![1.0, 0.0],
                },
            )
            .unwrap();
        assert_eq!(store.len(), 1);

        store.remove(10).unwrap();
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn test_mock_empty_store() {
        let store = MockSemanticStore::new();
        assert!(store.is_empty());
        let results = store
            .search(
                &TestLoad {
                    features: vec![1.0, 0.0],
                },
                5,
            )
            .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_insert_without_features_returns_error() {
        struct NoFeatures;
        impl FihLoad for NoFeatures {
            fn content(&self, _id: u32) -> Option<Vec<u8>> {
                None
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

        let mut store = MockSemanticStore::new();
        let result = store.insert(10, &NoFeatures);
        assert!(result.is_err());
    }
}
