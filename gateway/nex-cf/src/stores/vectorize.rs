// ── CfVectorizeStore: Cloudflare Vectorize-backed SemanticStore ──────
//
// Implements SemanticStore over Cloudflare Vectorize with Workers AI
// embedding.  Because SemanticStore is sync but Vectorize REST calls are
// async, this implementation uses a two-layer design:
//
//   Sync layer  — local text buffer that satisfies the trait contract
//   Async layer — methods that actually call Vectorize + Workers AI
//
// The local buffer serves as both a write cache and a fallback search
// mechanism.  Before each search, pending inserts are flushed to the
// Vectorize index so the next query sees the latest data.
//
// Usage:
//   1. Construct with CfVectorizeStore::new(env).
//   2. Register via storage.register_semantic_store(Box::new(store)).
//   3. Call sync_to_vectorize() from the queue handler after ingest.
//   4. Call search_vectorize_async() from the HTTP search handler.
//
// Workers AI model: @cf/baai/bge-small-en-v1.5 (384-dim).

use nex::storage::semantic::{Query, RecordLoad, SemanticStore};
use serde::Deserialize;
use worker::*;
use worker::js_sys::{Array, Float32Array, Function as JsFunction, Object, Reflect};
use worker::wasm_bindgen::{JsCast, JsValue};

/// Vectorize query match.
#[derive(Deserialize)]
struct VectorizeMatch {
    id: String,
    score: f32,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
}

/// Vectorize query response.
#[derive(Deserialize)]
struct VectorizeQueryResult {
    #[serde(default)]
    matches: Vec<VectorizeMatch>,
}

// ── Runtime helpers: call Workers AI for embedding ──────────────────────

/// Embed a single text string using Workers AI and return its embedding vector.
///
/// `@cf/baai/bge-small-en-v1.5` accepts `{ "text": "..." }` (single string) or
/// `{ "text": ["...", "..."] }` (array). Single mode returns `{ "data": [{ "embedding": [...] }] }`.
async fn embed_text(env: &Env, text: &str) -> Result<Vec<f32>> {
    let ai = env.ai("AI")?;
    // Single string (not array) for bge-small-en-v1.5
    let input = serde_json::json!({ "text": text });
    let result: serde_json::Value = ai
        .run("@cf/baai/bge-small-en-v1.5", &input)
        .await?;
    let data = result
        .get("data")
        .and_then(|d| d.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("embedding"))
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            Error::RustError(format!(
                "AI embedding: unexpected response shape: {}",
                serde_json::to_string(&result).unwrap_or_default()
            ))
        })?;

    let vec: Vec<f32> = data
        .iter()
        .filter_map(|v| v.as_f64().map(|f| f as f32))
        .collect();
    Ok(vec)
}

/// Embed multiple texts using Workers AI.
/// `@cf/baai/bge-small-en-v1.5` batch mode accepts `{ "text": ["...", "..."] }`.
async fn embed_texts(env: &Env, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
    let ai = env.ai("AI")?;
    let input = serde_json::json!({ "text": texts });
    let result: serde_json::Value = ai
        .run("@cf/baai/bge-small-en-v1.5", &input)
        .await?;
    let data = result
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| Error::RustError("AI embedding batch: unexpected response shape".into()))?;

    let mut embeddings = Vec::with_capacity(data.len());
    for entry in data {
        let vec: Vec<f32> = entry
            .get("embedding")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect())
            .unwrap_or_default();
        embeddings.push(vec);
    }
    Ok(embeddings)
}

/// Check if a Vectorize binding is available.
fn has_vectorize_binding(env: &Env) -> bool {
    js_sys::Reflect::get(env.as_ref(), &JsValue::from("SEMANTIC_INDEX"))
        .map(|v| !v.is_undefined())
        .unwrap_or(false)
}

// ── Global static buffer (CF Workers single-threaded) ────────────────
//
// Shared by all CfVectorizeStore instances. FihStorage owns one Box<dyn SemanticStore>,
// while the fetch handler keeps a &'static reference via PROD_VECTORIZE. Both read/write
// the same buffer, so semantic_insert and sync_to_vectorize access the same data.

static mut VECTORIZE_BUFFER: Vec<(u32, String)> = Vec::new();

/// Returns a mutable raw pointer to the global buffer.
///
/// # Safety
///
/// CF Workers is single-threaded, so concurrent access is impossible.
/// Callers must ensure no other `&mut` or `&` reference is live simultaneously.
fn buffer() -> *mut Vec<(u32, String)> {
    core::ptr::addr_of_mut!(VECTORIZE_BUFFER)
}

/// Clear the global buffer. Should only be used in tests or at initialization.
fn clear_buffer() {
    unsafe { (*buffer()).clear(); }
}

// ── CfVectorizeStore ────────────────────────────────────────────────────

/// Cloudflare Vectorize-backed semantic store with global static buffer.
pub struct CfVectorizeStore {
    env: Option<worker::Env>,
}

impl std::fmt::Debug for CfVectorizeStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // SAFETY: CF Workers single-threaded.
        let len = unsafe { (*buffer()).len() };
        f.debug_struct("CfVectorizeStore")
            .field("buffer_len", &len)
            .finish()
    }
}

impl CfVectorizeStore {
    /// Create a new CfVectorizeStore with the given Workers Env.
    ///
    /// The Env must have bindings for:
    ///   - `AI` — Workers AI binding (for text embedding)
    ///   - `SEMANTIC_INDEX` — Vectorize index binding
    ///
    /// If a binding is missing, the store degrades gracefully:
    ///   - Without Vectorize: operates as local-only (like InMemoryBm25)
    ///   - Without AI: local-only fallback
    pub fn new(env: worker::Env) -> Self {
        Self {
            env: Some(env),
        }
    }

    /// Flush all buffered inserts to the Vectorize index asynchronously.
    ///
    /// For each buffered (id, text) pair:
    ///   1. Embed text via Workers AI
    ///   2. Upsert the vector + metadata to Vectorize
    ///
    /// After successful flush, the buffer is NOT cleared (it serves as
    /// local fallback) but the synced flag is set to true.
    pub async fn sync_to_vectorize(&self) -> Result<()> {
        if unsafe { (*buffer()).is_empty() } {
            worker::console_log!("[CfVectorizeStore] sync: buffer empty, nothing to sync");
            return Ok(());
        }

        let buf = unsafe { &*buffer() };
        let texts: Vec<String> = buf.iter().map(|(_, t)| t.clone()).collect();
        let ids: Vec<u32> = buf.iter().map(|(id, _)| *id).collect();
        worker::console_log!("[CfVectorizeStore] sync: {} texts to embed", ids.len());

        let env = match self.env.as_ref() {
            Some(e) => e,
            None => {
                worker::console_log!("[CfVectorizeStore] sync: no env, skipping");
                return Ok(());
            }
        };
        let binding = match js_sys::Reflect::get(env.as_ref(), &JsValue::from("SEMANTIC_INDEX")) {
            Ok(v) if !v.is_undefined() => v,
            _ => {
                worker::console_log!("[CfVectorizeStore] SEMANTIC_INDEX binding not found or undefined");
                return Ok(());
            }
        };

        // Embed each text individually (avoid batch API format issues)
        let mut embeddings: Vec<Vec<f32>> = Vec::with_capacity(texts.len());
        for t in &texts {
            match embed_text(env, t).await {
                Ok(v) => embeddings.push(v),
                Err(e) => {
                    worker::console_log!("[CfVectorizeStore] embed error for '{}': {}", t, e);
                    return Err(e);
                }
            }
        }
        worker::console_log!("[CfVectorizeStore] sync: embedded {} texts", texts.len());

        // Build JS array of vector objects for Vectorize upsert
        let vectors = Array::new();
        for (id, embedding) in ids.iter().zip(embeddings.iter()) {
            let vector_obj = Object::new();

            // id: string (must match Vectorize's id field)
            let id_str = format!("f_{}", id);
            Reflect::set(&vector_obj, &JsValue::from("id"), &JsValue::from(&id_str))
                .unwrap_or_default();

            // values: Float32Array of embedding
            let typed_array = Float32Array::new_with_length(embedding.len() as u32);
            for (i, &val) in embedding.iter().enumerate() {
                typed_array.set_index(i as u32, val);
            }
            Reflect::set(
                &vector_obj,
                &JsValue::from("values"),
                &JsValue::from(typed_array.buffer()),
            )
            .unwrap_or_default();

            // metadata: { idx: number }
            let metadata = Object::new();
            Reflect::set(&metadata, &JsValue::from("idx"), &JsValue::from(*id))
                .unwrap_or_default();
            Reflect::set(
                &vector_obj,
                &JsValue::from("metadata"),
                &JsValue::from(metadata),
            )
            .unwrap_or_default();

            vectors.push(&vector_obj);
        }

        // Call binding.insert(vectors)
        let insert_fn = match Reflect::get(&binding, &JsValue::from("insert")) {
            Ok(f) if f.is_function() => f,
            _ => {
                console_log!("[CfVectorizeStore] insert method not found on binding");
                return Err(Error::RustError("Vectorize insert method not found".into()));
            }
        };

        let insert_fn_ref: &JsFunction = insert_fn.dyn_ref().ok_or_else(|| {
            Error::RustError("Vectorize insert: not a function".into())
        })?;

        let args = Array::new();
        args.push(&vectors);
        let promise: worker::js_sys::Promise = insert_fn_ref
            .apply(&binding, &args)
            .map_err(|e| {
                Error::RustError(format!("Vectorize insert call failed: {:?}", e))
            })?
            .into();
        worker::wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .map_err(|e| {
                Error::RustError(format!("Vectorize insert promise rejected: {:?}", e))
            })?;

        console_log!(
            "[CfVectorizeStore] synced {} vectors",
            ids.len()
        );

        Ok(())
    }

    /// Search the Vectorize index with a text query asynchronously.
    ///
    /// Steps:
    ///   1. Embed query text via Workers AI
    ///   2. Query Vectorize index
    ///   3. Return (id, score) pairs
    ///
    /// If Vectorize binding or AI is unavailable, falls back to local
    /// search (word-overlap scoring).
    pub async fn search_vectorize_async(&self, query_text: &str, top_k: usize) -> Result<Vec<(u32, f32)>> {
        if query_text.trim().is_empty() {
            return Ok(Vec::new());
        }

        // Try Vectorize search
        let env = match self.env.as_ref() {
            Some(e) => e,
            None => return Ok(self.local_search(query_text, top_k)),
        };
        let binding = match js_sys::Reflect::get(env.as_ref(), &JsValue::from("SEMANTIC_INDEX")) {
            Ok(v) if !v.is_undefined() => v,
            _ => {
                // Fall back to local search
                return Ok(self.local_search(query_text, top_k));
            }
        };

        // Embed the query text
        let query_vec = match embed_text(env, query_text).await {
            Ok(v) => v,
            Err(e) => {
                console_log!("[CfVectorizeStore] search embed error: {e}, falling back to local");
                return Ok(self.local_search(query_text, top_k));
            }
        };

        // Build the query call: binding.query(queryVector, topK, options?)
        let query_fn = match Reflect::get(&binding, &JsValue::from("query")) {
            Ok(f) if f.is_function() => f,
            _ => {
                console_log!("[CfVectorizeStore] query method not found, falling back to local");
                return Ok(self.local_search(query_text, top_k));
            }
        };

        // Convert query vector to Float32Array buffer
        let typed_array = js_sys::Float32Array::new_with_length(query_vec.len() as u32);
        for (i, &val) in query_vec.iter().enumerate() {
            typed_array.set_index(i as u32, val);
        }

        let query_fn_ref: &JsFunction = query_fn.dyn_ref().ok_or_else(|| {
            Error::RustError("Vectorize query: not a function".into())
        })?;

        let args = Array::new();
        args.push(&JsValue::from(typed_array.buffer()));
        args.push(&JsValue::from(top_k as f64));

        // Options: { returnValues: false, returnMetadata: "all" }
        let options = Object::new();
        Reflect::set(&options, &JsValue::from("returnValues"), &JsValue::FALSE)
            .unwrap_or_default();
        Reflect::set(
            &options,
            &JsValue::from("returnMetadata"),
            &JsValue::from("all"),
        )
        .unwrap_or_default();
        args.push(&options);

        let promise: worker::js_sys::Promise = query_fn_ref
            .apply(&binding, &args)
            .map_err(|e| {
                Error::RustError(format!("Vectorize query call failed: {:?}", e))
            })?
            .into();
        let result_val = worker::wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .map_err(|e| {
                Error::RustError(format!("Vectorize query promise rejected: {:?}", e))
            })?;

        // Parse the JSON result
        let result_str = js_sys::JSON::stringify(&result_val)
            .map(|s| s.as_string().unwrap_or_default())
            .unwrap_or_default();

        if let Ok(parsed) = serde_json::from_str::<VectorizeQueryResult>(&result_str) {
            let results: Vec<(u32, f32)> = parsed
                .matches
                .into_iter()
                .filter_map(|m| {
                    // Extract idx from metadata, or from the id string
                    let idx = m
                        .metadata
                        .as_ref()
                        .and_then(|meta| meta.get("idx").and_then(|v| v.as_u64()).map(|v| v as u32))
                        .or_else(|| {
                            // Fallback: parse from id string "f_{idx}"
                            m.id.strip_prefix("f_")
                                .and_then(|s| s.parse::<u32>().ok())
                        });
                    idx.map(|id| (id, m.score))
                })
                .collect();
            return Ok(results);
        }

        // Fallback to local search if JSON parse fails
        console_log!("[CfVectorizeStore] query JSON parse failed, falling back to local");
        Ok(self.local_search(query_text, top_k))
    }

    /// Check whether a Vectorize binding is configured in the environment.
    pub fn vectorize_available(&self) -> bool {
        self.env.as_ref().is_some_and(has_vectorize_binding)
    }

    // ── Local fallback search ────────────────────────────────────────
    //
    // Simple word-overlap scoring used when Vectorize is unavailable.
    // Mirrors the InMemoryBm25 algorithm but with fewer dependencies
    // (no IDF weighting).

    fn local_search(&self, query_text: &str, top_k: usize) -> Vec<(u32, f32)> {
        let buffer = unsafe { &*buffer() };
        if buffer.is_empty() {
            return Vec::new();
        }

        let query_terms: Vec<String> = query_text
            .to_lowercase()
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        if query_terms.is_empty() {
            return Vec::new();
        }

        let mut scored: Vec<(u32, f32)> = buffer
            .iter()
            .map(|(id, text)| {
                let doc_lower = text.to_lowercase();
                let doc_words: Vec<&str> = doc_lower.split_whitespace().collect();
                let total_words = doc_words.len() as f64;
                if total_words == 0.0 {
                    return (*id, 0.0);
                }

                let mut score = 0.0f64;
                for qt in &query_terms {
                    let tf = doc_words.iter().filter(|w| *w == qt).count() as f64;
                    if tf > 0.0 {
                        // Simple TF with normalization
                        score += tf / (total_words).sqrt();
                    }
                }
                (*id, score as f32)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k);
        scored
    }
}

// ── SemanticStore trait implementation (async, local-only) ─────────────

#[async_trait::async_trait(?Send)]
impl SemanticStore for CfVectorizeStore {
    async fn insert(&mut self, id: u32, load: &dyn RecordLoad) -> Result<(), String> {
        let text = load
            .text(id)
            .ok_or_else(|| format!("CfVectorizeStore: no text for id {id}"))?;
        unsafe { (*buffer()).push((id, text)); }
        Ok(())
    }

    async fn search(&self, query: &dyn Query, top_k: usize) -> Result<Vec<(u32, f32)>, String> {
        let qt = match query.text() {
            Some(t) if !t.trim().is_empty() => t,
            _ => return Ok(Vec::new()),
        };
        Ok(self.local_search(&qt, top_k))
    }

    async fn remove(&mut self, id: u32) -> Result<(), String> {
        unsafe { (*buffer()).retain(|(i, _)| *i != id); }
        Ok(())
    }

    fn len(&self) -> usize {
        unsafe { (*buffer()).len() }
    }
}

// ── Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use nex::storage::semantic::record::RecordLoad;

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

    /// Helper to create a CfVectorizeStore for testing (no Env).
    fn make_test_store() -> CfVectorizeStore {
        // Clear the global static buffer so each test starts fresh.
        clear_buffer();
        CfVectorizeStore {
            env: None,
        }
    }

    #[test]
    fn test_local_search_exact_match() {
        futures_executor::block_on(async {
            let mut store = make_test_store();
            store
                .insert(1, &TestLoad { text: "Rust is a systems programming language".into() })
                .await
                .unwrap();
            store
                .insert(2, &TestLoad { text: "Python is a general purpose language".into() })
                .await
                .unwrap();
            store
                .insert(3, &TestLoad { text: "JavaScript runs in the browser".into() })
                .await
                .unwrap();

            let results = store
                .search(&TestQuery { text: "Rust programming".into() }, 5)
                .await
                .unwrap();
            assert!(!results.is_empty(), "expected at least one match");
            assert_eq!(results[0].0, 1, "expected id=1 to be top match");
            let results2 = store
                .search(&TestQuery { text: "browser".into() }, 5)
                .await
                .unwrap();
            assert!(!results2.is_empty(), "expected match for browser");
            assert_eq!(results2[0].0, 3, "expected id=3 to be top match for browser");
        });
    }

    #[test]
    fn test_local_search_no_match() {
        futures_executor::block_on(async {
            let mut store = make_test_store();
            store
                .insert(1, &TestLoad { text: "Rust is a systems programming language".into() })
                .await
                .unwrap();
            let results = store
                .search(&TestQuery { text: "quantum physics".into() }, 5)
                .await
                .unwrap();
            assert!(results.is_empty() || results[0].1 == 0.0);
        });
    }

    #[test]
    fn test_remove() {
        futures_executor::block_on(async {
            let mut store = make_test_store();
            store
                .insert(1, &TestLoad { text: "Rust language".into() })
                .await
                .unwrap();
            assert_eq!(store.len(), 1);
            store.remove(1).await.unwrap();
            assert_eq!(store.len(), 0);
        });
    }

    #[test]
    fn test_empty_store() {
        futures_executor::block_on(async {
            let store = make_test_store();
            assert!(store.is_empty());
            let results = store
                .search(&TestQuery { text: "anything".into() }, 5)
                .await
                .unwrap();
            assert!(results.is_empty());
        });
    }

    #[test]
    fn test_insert_duplicate_id() {
        futures_executor::block_on(async {
            let mut store = make_test_store();
            store
                .insert(1, &TestLoad { text: "first".into() })
                .await
                .unwrap();
            store
                .insert(1, &TestLoad { text: "second".into() })
                .await
                .unwrap();
            // Both entries kept (Vectorize upserts by id; local buffer is append-only)
            assert_eq!(store.len(), 2);
        });
    }

    #[test]
    fn test_top_k_limits_results() {
        futures_executor::block_on(async {
            let mut store = make_test_store();
            for i in 1..=10 {
                store
                    .insert(i, &TestLoad { text: format!("document number {i}") })
                    .await
                    .unwrap();
            }
            let results = store
                .search(&TestQuery { text: "document".into() }, 3)
                .await
                .unwrap();
            assert!(results.len() <= 3, "expected at most 3 results");
        });
    }

    #[test]
    fn test_bm25_matches_inmemory_bm25_pattern() {
        futures_executor::block_on(async {
            let mut store = make_test_store();
            store
                .insert(1, &TestLoad { text: "Graph Neural Networks process graph-structured data through message-passing between nodes".into() })
                .await
                .unwrap();
            store
                .insert(2, &TestLoad { text: "Transformer models use self-attention mechanisms to process sequential data".into() })
                .await
                .unwrap();
            store
                .insert(3, &TestLoad { text: "Gradient descent optimizes neural network parameters".into() })
                .await
                .unwrap();

            let results = store
                .search(&TestQuery { text: "Graph Neural".into() }, 5)
                .await
                .unwrap();
            assert!(!results.is_empty(), "expected match for Graph Neural");
            assert_eq!(results[0].0, 1, "expected document 1 about GNN to be top match");
        });
    }
}
