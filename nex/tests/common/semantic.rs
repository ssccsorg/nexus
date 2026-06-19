// Mock SemanticStore implementations for testing similarity search.
//
// Each integration test binary compiles this module but may not use every mock.
// The `dead_code` allow is intentional: this is a shared mock library.
#![allow(dead_code)]

use nex::storage::semantic::{Query, RecordLoad, SemanticStore};
use std::collections::HashMap;
use std::fmt::Debug;

// ── Mock SemanticStore (cosine vector) ───────────────────────────────────

/// Mock in-memory SemanticStore using brute-force cosine similarity.
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

impl Default for MockSemanticStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl SemanticStore for MockSemanticStore {
    async fn insert(&mut self, id: u32, load: &dyn RecordLoad) -> Result<(), String> {
        let features = load
            .features(id)
            .ok_or_else(|| "no features available".to_string())?;
        self.ids.push(id);
        self.vectors.push(features);
        Ok(())
    }

    async fn search(&self, query: &dyn Query, top_k: usize) -> Result<Vec<(u32, f32)>, String> {
        let query_vec = query
            .features()
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

    async fn remove(&mut self, id: u32) -> Result<(), String> {
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

// ── Mock BM25 text-based SemanticStore ───────────────────────────────────

/// A brute-force BM25-like semantic store operating on text content.
#[derive(Debug)]
pub struct MockBm25Store {
    ids: Vec<u32>,
    texts: Vec<String>,
}

impl MockBm25Store {
    pub fn new() -> Self {
        Self {
            ids: Vec::new(),
            texts: Vec::new(),
        }
    }
}

impl Default for MockBm25Store {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait(?Send)]
impl SemanticStore for MockBm25Store {
    async fn insert(&mut self, id: u32, load: &dyn RecordLoad) -> Result<(), String> {
        let text = load
            .text(id)
            .ok_or_else(|| "no text available".to_string())?;
        self.ids.push(id);
        self.texts.push(text);
        Ok(())
    }

    async fn search(&self, query: &dyn Query, top_k: usize) -> Result<Vec<(u32, f32)>, String> {
        let query_text = query.text().ok_or_else(|| "no query text".to_string())?;

        if self.ids.is_empty() {
            return Ok(Vec::new());
        }

        let query_terms: Vec<String> = query_text
            .to_lowercase()
            .split_whitespace()
            .map(|t| t.to_string())
            .collect();

        if query_terms.is_empty() {
            return Ok(Vec::new());
        }

        let total_docs = self.texts.len();
        let avg_doc_len: f64 = self
            .texts
            .iter()
            .map(|t| t.split_whitespace().count() as f64)
            .sum::<f64>()
            / total_docs as f64;

        let mut doc_freq: HashMap<String, usize> = HashMap::new();
        for term in &query_terms {
            let count = self
                .texts
                .iter()
                .filter(|doc| doc.to_lowercase().split_whitespace().any(|w| w == term))
                .count();
            doc_freq.insert(term.clone(), count);
        }

        let k1 = 1.2;
        let b = 0.75;

        let mut scores: Vec<(u32, f32)> = self
            .ids
            .iter()
            .zip(self.texts.iter())
            .map(|(&id, doc_text)| {
                let doc_len = doc_text.split_whitespace().count() as f64;
                let mut score = 0.0;
                for term in &query_terms {
                    let tf = doc_text
                        .to_lowercase()
                        .split_whitespace()
                        .filter(|w| w == term)
                        .count() as f64;
                    if tf == 0.0 {
                        continue;
                    }
                    let df = *doc_freq.get(term).unwrap_or(&0) as f64;
                    let idf = ((total_docs as f64 - df + 0.5) / (df + 0.5) + 1.0).ln();
                    score += idf * (tf * (k1 + 1.0))
                        / (tf + k1 * (1.0 - b + b * doc_len / avg_doc_len.max(1.0)));
                }
                (id, score as f32)
            })
            .collect();

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores.truncate(top_k);
        Ok(scores)
    }

    async fn remove(&mut self, id: u32) -> Result<(), String> {
        if let Some(pos) = self.ids.iter().position(|&i| i == id) {
            self.ids.remove(pos);
            self.texts.remove(pos);
        }
        Ok(())
    }

    fn len(&self) -> usize {
        self.ids.len()
    }
}

// ── FeatureLoad: test/utility RecordLoad+Query implementation ───────────

/// A `RecordLoad` + `Query` implementation that carries a feature vector
/// and an optional text string.
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

impl RecordLoad for FeatureLoad {
    fn content(&self, _id: u32) -> Option<Vec<u8>> {
        self.text.as_ref().map(|t| t.as_bytes().to_vec())
    }
    fn text(&self, _id: u32) -> Option<String> {
        self.text.clone()
    }
    fn features(&self, _id: u32) -> Option<Vec<f32>> {
        Some(self.features.clone())
    }
}

impl Query for FeatureLoad {
    fn features(&self) -> Option<Vec<f32>> {
        Some(self.features.clone())
    }
    fn text(&self) -> Option<String> {
        self.text.clone()
    }
}
