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
//
// nex core defines only the trait. External crates provide impls.
// Mock implementations live in `tests/common/mod.rs`.

use std::fmt::Debug;

/// Lookup handle for loading Fih data by record ID.
///
/// Used by `SemanticStore::insert()` to retrieve the data it needs
/// (feature vectors, text, origin, etc.) for a given record.
pub trait FihLoad {
    /// Load content bytes for a record by its coord index.
    fn content(&self, id: u32) -> Option<Vec<u8>>;

    /// Load content decoded as UTF-8 text.
    fn text(&self, id: u32) -> Option<String> {
        self.content(id)
            .and_then(|bytes| String::from_utf8(bytes).ok())
    }

    /// Load f32 feature vector, if the record has one stored.
    ///
    /// The core `FihStorage` implementation returns `None` because feature
    /// vectors are not stored inline. External embedding services (agent layer)
    /// should override this via a custom `FihLoad` wrapper that calls an
    /// embedding API and caches the result.
    fn features(&self, id: u32) -> Option<Vec<f32>>;

    /// Load the origin string for a fact record.
    fn origin(&self, id: u32) -> Option<String>;

    /// Load the creator string for a fact or intent record.
    fn creator(&self, id: u32) -> Option<String>;
}

/// Query handle for similarity search.
///
/// Used by `SemanticStore::search()`. Unlike `FihLoad`, it carries no
/// record ID — only the query data needed to find similar records.
/// Each implementation calls only the accessor it needs (e.g.
/// `features()` for vector search, `text()` for BM25).
pub trait FihQuery {
    /// Query as f32 feature vector.
    fn features(&self) -> Option<Vec<f32>>;

    /// Query as UTF-8 text.
    fn text(&self) -> Option<String>;
}

/// Semantic feature store for similarity search.
///
/// Maps semantic features to record IDs. Used by FihCoord as another
/// index axis alongside by_origin, by_creator, by_status, etc.
///
/// Each implementation decides which data to extract via `FihLoad`.
///
/// Multiple store implementations can coexist in the same binary,
/// each using a different strategy (vector cosine, BM25 text, ngram,
/// etc.) without the core knowing which one is in use.
pub trait SemanticStore: Debug {
    /// Insert a record into the store. The implementation calls
    /// `load` to retrieve only the data it needs.
    fn insert(&mut self, id: u32, load: &dyn FihLoad) -> Result<(), String>;

    /// Search for the top_k most similar records using the query handle.
    fn search(&self, query: &dyn FihQuery, top_k: usize) -> Result<Vec<(u32, f32)>, String>;

    /// Remove a record ID from the store.
    fn remove(&mut self, id: u32) -> Result<(), String>;

    /// Number of records stored.
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ── FeatureLoad: test/utility FihLoad+FihQuery implementation ───────────

/// A `FihLoad` + `FihQuery` implementation that carries a feature vector
/// and an optional text string.
///
/// This is a testing utility available to both the crate's own tests and
/// external integration tests, demonstrating the flashlight pattern with
/// both vector and text data.
///
/// Hidden from public documentation to avoid external consumers depending
/// on what is ultimately a test helper.
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct FeatureLoad {
    features: Vec<f32>,
    text: Option<String>,
}

impl FeatureLoad {
    pub fn new(features: Vec<f32>, text: Option<String>) -> Self {
        Self { features, text }
    }
}

impl FihLoad for FeatureLoad {
    fn content(&self, _id: u32) -> Option<Vec<u8>> {
        self.text.as_ref().map(|t| t.as_bytes().to_vec())
    }
    fn text(&self, _id: u32) -> Option<String> {
        self.text.clone()
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

impl FihQuery for FeatureLoad {
    fn features(&self) -> Option<Vec<f32>> {
        Some(self.features.clone())
    }
    fn text(&self) -> Option<String> {
        self.text.clone()
    }
}
