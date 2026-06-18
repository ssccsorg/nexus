// ── SemanticStore: semantic feature store for similarity search ────────
//
// Another kind of Store, like EntityStore for identity-based access.
// SemanticStore associates record IDs with semantic features for
// similarity-based retrieval.
//
// The trait is designed as a thin "flashlight" interface: the core (FihCoord)
// provides only a lookup handle (`RecordLoad`), and each external implementation
// decides which data it needs (text, embeddings, graph relations, etc.)
// by requesting it through the handle. This keeps the core agnostic to
// any specific methodology — vector, BM25, ngram, hypergraph, LLM reranker.
//
// Implementations (plug-in via USB hub pattern, living outside nex core):
//   - storage/semantic/vector/  — HNSW-based ANN vector search (external)
//   - storage/semantic/ngram/   — ngram-based fuzzy matching (external)
//
// nex core defines only the traits. External crates provide impls.

pub mod fih;
pub mod record;

pub use record::{Query, RecordLoad};

use std::fmt::Debug;

/// Semantic feature store for similarity search.
///
/// Maps semantic features to record IDs. Used by FihCoord as another
/// index axis alongside by_origin, by_creator, by_status, etc.
///
/// Each implementation decides which data to extract via `RecordLoad`.
///
/// Multiple store implementations can coexist in the same binary,
/// each using a different strategy (vector cosine, BM25 text, ngram,
/// etc.) without the core knowing which one is in use.
pub trait SemanticStore: Debug {
    /// Insert a record into the store. The implementation calls
    /// `load` to retrieve only the data it needs.
    fn insert(&mut self, id: u32, load: &dyn RecordLoad) -> Result<(), String>;

    /// Search for the top_k most similar records using the query handle.
    fn search(&self, query: &dyn Query, top_k: usize) -> Result<Vec<(u32, f32)>, String>;

    /// Remove a record ID from the store.
    fn remove(&mut self, id: u32) -> Result<(), String>;

    /// Number of records stored.
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
