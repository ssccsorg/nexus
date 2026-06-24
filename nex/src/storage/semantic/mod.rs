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

// ── Platform-adaptive async trait bounds ───────────────────────────
//
// On native/WASIX: require Send + Sync so FihStorage is Send + Sync.
// On wasm32-unknown-unknown: use ?Send (single-threaded).
// External implementations use the same trait regardless of platform.

/// Semantic feature store for similarity search.
#[cfg(not(target_arch = "wasm32"))]
#[async_trait::async_trait]
pub trait SemanticStore: Debug + Send + Sync {
    async fn insert(&mut self, id: u32, load: &dyn RecordLoad) -> Result<(), String>;
    async fn search(&self, query: &dyn Query, top_k: usize) -> Result<Vec<(u32, f32)>, String>;
    async fn remove(&mut self, id: u32) -> Result<(), String>;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(target_arch = "wasm32")]
#[async_trait::async_trait(?Send)]
pub trait SemanticStore: Debug {
    async fn insert(&mut self, id: u32, load: &dyn RecordLoad) -> Result<(), String>;
    async fn search(&self, query: &dyn Query, top_k: usize) -> Result<Vec<(u32, f32)>, String>;
    async fn remove(&mut self, id: u32) -> Result<(), String>;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Convenience type alias for storing SemanticStore in containers.
#[cfg(not(target_arch = "wasm32"))]
pub type DynSemanticStore = Box<dyn SemanticStore + Send>;

#[cfg(target_arch = "wasm32")]
pub type DynSemanticStore = Box<dyn SemanticStore>;
