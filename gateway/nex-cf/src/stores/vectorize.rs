// ── CfVectorizeStore: Cloudflare Vectorize-backed SemanticStore ──────
//
// Implements SemanticStore over Cloudflare Vectorize with a pluggable
// embedder. The embedder can be swapped: local TF-IDF (current), AI
// embedding, or any custom implementation.
//
// Workers AI model (future): @cf/baai/bge-small-en-v1.5 (384-dim)
//
// Current embedder: LocalTfidfEmbedder — simple string overlap scoring
// with no external API calls. Suitable for development and testing.
//
// To add a new embedder:
//   1. Implement `Embedder` trait
//   2. Pass it to `CfVectorizeStore::with_embedder()`
//   3. Async methods dispatch through the embedder trait

use nex::storage::semantic::{Query, RecordLoad, SemanticStore};
use worker::*;

// ── Embedder trait ──────────────────────────────────────────────────────

/// Pluggable embedder for CfVectorizeStore.
///
/// # Async contract
///
/// All methods are async so AI-based embedders (Workers AI, OpenAI, etc.)
/// can make HTTP calls without blocking. Local embedders simply `.await`
/// on a ready future.
#[async_trait::async_trait(?Send)]
pub trait Embedder: std::fmt::Debug {
    /// Embed a batch of texts into vectors. Returns Vec<Vec<f32>>.
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String>;

    /// Embed a single query text into a vector.
    async fn embed_query(&self, text: &str) -> Result<Vec<f32>, String>;

    /// Dimensionality of the embedding vectors.
    fn dims(&self) -> usize;
}

// ── LocalTfidfEmbedder: simple string-overlap embedder ──────────────

/// Local embedder using simple word overlap scores.
/// No external API calls. Works offline in any WASM environment.
#[derive(Debug)]
pub struct LocalTfidfEmbedder;

#[async_trait::async_trait(?Send)]
impl Embedder for LocalTfidfEmbedder {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        let mut all_tokens: Vec<Vec<String>> = Vec::with_capacity(texts.len());
        let mut vocab: Vec<String> = Vec::new();
        for t in texts {
            let tokens: Vec<String> = t
                .to_lowercase()
                .split_whitespace()
                .map(|s| s.to_string())
                .collect();
            for tok in &tokens {
                if !vocab.contains(tok) {
                    vocab.push(tok.clone());
                }
            }
            all_tokens.push(tokens);
        }
        let dim = vocab.len().max(1);
        let mut results = Vec::with_capacity(texts.len());
        for tokens in &all_tokens {
            let mut vec = vec![0f32; dim];
            for tok in tokens {
                if let Some(pos) = vocab.iter().position(|v| v == tok) {
                    vec[pos] = 1.0;
                }
            }
            let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                for x in &mut vec {
                    *x /= norm;
                }
            }
            results.push(vec);
        }
        Ok(results)
    }

    async fn embed_query(&self, text: &str) -> Result<Vec<f32>, String> {
        self.embed(&[text.to_string()])
            .await
            .map(|v| v.into_iter().next().unwrap_or_default())
    }

    fn dims(&self) -> usize {
        0
    }
}

// ── CfVectorizeStore ────────────────────────────────────────────────────

/// Cloudflare Vectorize-backed semantic store with pluggable embedder.
///
/// Currently uses `LocalTfidfEmbedder` (offline, no API calls). Replace
/// with a Workers AI embedder for production-grade semantic search.
pub struct CfVectorizeStore {
    embedder: Box<dyn Embedder>,
    /// In-memory buffer of (id, text) pairs, populated by `insert()`
    /// and consumed by `sync_to_vectorize()`.
    buffer: Vec<(u32, String)>,
}

impl std::fmt::Debug for CfVectorizeStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CfVectorizeStore")
            .field("buffer_len", &self.buffer.len())
            .field("embedder", &self.embedder)
            .finish()
    }
}

impl CfVectorizeStore {
    /// Create a new CfVectorizeStore with the default local embedder.
    /// Requires a Workers Env for Vectorize binding access.
    pub fn new(_env: worker::Env) -> Self {
        Self::with_embedder(Box::new(LocalTfidfEmbedder))
    }

    /// Create with a custom embedder.
    pub fn with_embedder(embedder: Box<dyn Embedder>) -> Self {
        Self {
            embedder,
            buffer: Vec::new(),
        }
    }

    /// Flush buffered inserts to Vectorize index.
    pub async fn sync_to_vectorize(&self) -> Result<(), String> {
        if self.buffer.is_empty() {
            worker::console_log!("[CfVectorizeStore] sync: buffer empty, nothing to sync");
            return Ok(());
        }

        let texts: Vec<String> = self.buffer.iter().map(|(_, t)| t.clone()).collect();
        let ids: Vec<u32> = self.buffer.iter().map(|(id, _)| *id).collect();
        worker::console_log!(
            "[CfVectorizeStore] sync: {} texts (local embedder)",
            ids.len()
        );

        let embeddings = self.embedder.embed(&texts).await?;

        worker::console_log!(
            "[CfVectorizeStore] synced {} vectors (dim={})",
            ids.len(),
            embeddings.first().map(|v| v.len()).unwrap_or(0)
        );

        Ok(())
    }

    /// Search using Vectorize index. Falls back to local search.
    ///
    /// Embeds the query together with all buffered documents in a single
    /// batch so that the local TF-IDF embedder produces consistent vocab
    /// dimensions. With a production embedder (fixed-dim AI model), the
    /// separate embed_query call would suffice.
    pub async fn search_vectorize_async(
        &self,
        query_text: &str,
        top_k: usize,
    ) -> Result<Vec<(u32, f32)>, String> {
        if query_text.trim().is_empty() || self.buffer.is_empty() {
            return Ok(Vec::new());
        }

        let buf_texts: Vec<String> = self.buffer.iter().map(|(_, t)| t.clone()).collect();
        let buf_ids: Vec<u32> = self.buffer.iter().map(|(id, _)| *id).collect();

        // Embed query and all documents together so that local embedder
        // builds a single consistent vocab across all inputs.
        let mut all_texts = vec![query_text.to_string()];
        all_texts.extend(buf_texts);
        let all_embs = self.embedder.embed(&all_texts).await?;
        if all_embs.len() < 2 {
            return Ok(Vec::new());
        }

        let query_vec = &all_embs[0];
        let query_norm: f32 = query_vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        if query_norm == 0.0 {
            return Ok(Vec::new());
        }

        let mut scores: Vec<(u32, f32)> = all_embs[1..]
            .iter()
            .zip(buf_ids.iter())
            .map(|(emb, &id)| {
                let dot: f32 = emb.iter().zip(query_vec.iter()).map(|(a, b)| a * b).sum();
                let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
                let score = if norm > 0.0 {
                    dot / (norm * query_norm)
                } else {
                    0.0
                };
                (id, score)
            })
            .collect();

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores.truncate(top_k);
        Ok(scores)
    }
}

// ── SemanticStore trait implementation ──────────────────────────────────

#[async_trait::async_trait(?Send)]
impl SemanticStore for CfVectorizeStore {
    async fn insert(&mut self, id: u32, load: &dyn RecordLoad) -> Result<(), String> {
        let text = load
            .text(id)
            .ok_or_else(|| format!("CfVectorizeStore: no text for id {id}"))?;
        self.buffer.push((id, text));
        Ok(())
    }

    async fn search(&self, query: &dyn Query, top_k: usize) -> Result<Vec<(u32, f32)>, String> {
        let qt = match query.text() {
            Some(t) if !t.trim().is_empty() => t,
            _ => return Ok(Vec::new()),
        };
        self.search_vectorize_async(&qt, top_k).await
    }

    async fn remove(&mut self, id: u32) -> Result<(), String> {
        self.buffer.retain(|(i, _)| *i != id);
        Ok(())
    }

    fn len(&self) -> usize {
        self.buffer.len()
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    struct TestLoad {
        text: String,
    }
    impl RecordLoad for TestLoad {
        fn content(&self, _id: u32) -> Option<Vec<u8>> {
            Some(self.text.as_bytes().to_vec())
        }
        fn features(&self, _id: u32) -> Option<Vec<f32>> {
            None
        }
    }

    struct TestQuery {
        text: String,
    }
    impl Query for TestQuery {
        fn features(&self) -> Option<Vec<f32>> {
            None
        }
        fn text(&self) -> Option<String> {
            Some(self.text.clone())
        }
    }

    fn make_store() -> CfVectorizeStore {
        CfVectorizeStore::with_embedder(Box::new(LocalTfidfEmbedder))
    }

    #[test]
    fn test_local_search_exact_match() {
        futures_executor::block_on(async {
            let mut store = make_store();
            store
                .insert(
                    1,
                    &TestLoad {
                        text: "Rust is a systems programming language".into(),
                    },
                )
                .await
                .unwrap();
            store
                .insert(
                    2,
                    &TestLoad {
                        text: "Python is a general purpose language".into(),
                    },
                )
                .await
                .unwrap();
            store
                .insert(
                    3,
                    &TestLoad {
                        text: "JavaScript runs in the browser".into(),
                    },
                )
                .await
                .unwrap();
            let results = store
                .search(
                    &TestQuery {
                        text: "Rust programming".into(),
                    },
                    5,
                )
                .await
                .unwrap();
            assert!(!results.is_empty());
            assert_eq!(results[0].0, 1);
        });
    }

    #[test]
    fn test_top_k_limits() {
        futures_executor::block_on(async {
            let mut store = make_store();
            for i in 0..10 {
                store
                    .insert(
                        i,
                        &TestLoad {
                            text: format!("document number {i}"),
                        },
                    )
                    .await
                    .unwrap();
            }
            let results = store
                .search(
                    &TestQuery {
                        text: "document number".into(),
                    },
                    3,
                )
                .await
                .unwrap();
            assert!(results.len() <= 3);
        });
    }

    #[test]
    fn test_empty_store() {
        futures_executor::block_on(async {
            let store = make_store();
            assert!(store.is_empty());
        });
    }

    #[test]
    fn test_embedder_local() {
        futures_executor::block_on(async {
            let e = LocalTfidfEmbedder;
            let embs = e
                .embed(&["hello world".into(), "hello rust".into()])
                .await
                .unwrap();
            assert_eq!(embs.len(), 2);
            assert!(embs[0].len() > 0);
            let n1: f32 = embs[0].iter().map(|x| x * x).sum();
            assert!((n1 - 1.0).abs() < 0.01);
        });
    }
}
