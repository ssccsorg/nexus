// ── VectorStore: ANN vector storage abstraction ────────────────────────
//
// Another kind of Store, like HashMap for exact-match indexes and
// BTreeMap for range indexes. VectorStore maps embedding vectors to
// record IDs via Approximate Nearest Neighbor search.
//
// Implementations (plug-in via USB hub pattern):
//   - CfVectorize: Cloudflare Vectorize (WASM)
//   - UsearchIndex: local HNSW (native)
//   - MockVectorStore: in-memory brute force (testing)
//
// nex core defines only the trait. External crates provide impls.

/// ANN (Approximate Nearest Neighbor) vector storage.
///
/// Maps embedding vectors to record IDs. Used by FihCoord as another
/// index axis alongside by_origin, by_creator, by_status, etc.
pub trait VectorStore {
    /// Insert a vector with associated record ID.
    fn insert(&mut self, id: u32, vector: &[f32]) -> Result<(), String>;

    /// Search for top_k nearest neighbors by cosine similarity.
    fn search(&self, vector: &[f32], top_k: usize) -> Result<Vec<(u32, f32)>, String>;

    /// Remove a record ID from the index.
    fn remove(&mut self, id: u32) -> Result<(), String>;

    /// Number of vectors stored.
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Mock in-memory VectorStore for testing.
/// Brute-force cosine similarity search — O(N*d) per query.
pub struct MockVectorStore {
    ids: Vec<u32>,
    vectors: Vec<Vec<f32>>,
}

impl MockVectorStore {
    pub fn new() -> Self {
        Self {
            ids: Vec::new(),
            vectors: Vec::new(),
        }
    }
}

impl VectorStore for MockVectorStore {
    fn insert(&mut self, id: u32, vector: &[f32]) -> Result<(), String> {
        self.ids.push(id);
        self.vectors.push(vector.to_vec());
        Ok(())
    }

    fn search(&self, query: &[f32], top_k: usize) -> Result<Vec<(u32, f32)>, String> {
        if query.len() != self.vectors.first().map(|v| v.len()).unwrap_or(query.len()) {
            return Err("dimension mismatch".into());
        }
        let mut scores: Vec<(u32, f32)> = self
            .ids
            .iter()
            .zip(self.vectors.iter())
            .map(|(&id, vec)| {
                let dot: f32 = query.iter().zip(vec.iter()).map(|(a, b)| a * b).sum();
                let norm_q: f32 = query.iter().map(|x| x * x).sum::<f32>().sqrt();
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

impl Default for MockVectorStore {
    fn default() -> Self {
        Self::new()
    }
}
