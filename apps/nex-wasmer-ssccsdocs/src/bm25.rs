// InMemoryBm25: brute-force BM25 semantic store.
//
// Pure Rust BM25 implementation. No external dependencies or API calls.
// Suitable for <10K documents. For larger collections, add an external
// vector database adapter.

use nex::storage::semantic::{Query, RecordLoad, SemanticStore};
use std::collections::HashMap;

/// In-memory BM25 semantic store.
#[derive(Debug)]
pub struct InMemoryBm25 {
    ids: Vec<u32>,
    texts: Vec<String>,
}

impl InMemoryBm25 {
    pub fn new() -> Self {
        Self {
            ids: Vec::new(),
            texts: Vec::new(),
        }
    }
}

impl Default for InMemoryBm25 {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[async_trait::async_trait]
impl SemanticStore for InMemoryBm25 {
    async fn insert(&mut self, id: u32, load: &dyn RecordLoad) -> Result<(), String> {
        let text = load
            .text(id)
            .ok_or_else(|| "no text available".to_string())?;
        self.ids.push(id);
        self.texts.push(text);
        Ok(())
    }

    async fn search(&self, query: &dyn Query, top_k: usize) -> Result<Vec<(u32, f32)>, String> {
        let qt = match query.text() {
            Some(t) if !t.trim().is_empty() => t,
            _ => return Ok(Vec::new()),
        };
        if self.ids.is_empty() {
            return Ok(Vec::new());
        }
        let terms: Vec<String> = qt
            .to_lowercase()
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let n = self.texts.len();
        let avg_len: f64 = self
            .texts
            .iter()
            .map(|t| t.split_whitespace().count() as f64)
            .sum::<f64>()
            / n.max(1) as f64;

        let mut df: HashMap<&str, usize> = HashMap::new();
        for t in &terms {
            let count = self
                .texts
                .iter()
                .filter(|doc| doc.to_lowercase().split_whitespace().any(|w| w == t))
                .count();
            df.insert(t, count);
        }

        let k1 = 1.2;
        let b = 0.75;
        let mut scores: Vec<(u32, f32)> = self
            .ids
            .iter()
            .zip(self.texts.iter())
            .map(|(&id, doc)| {
                let dl = doc.split_whitespace().count() as f64;
                let mut score = 0.0;
                for t in &terms {
                    let tf = doc
                        .to_lowercase()
                        .split_whitespace()
                        .filter(|w| w == t)
                        .count() as f64;
                    if tf == 0.0 {
                        continue;
                    }
                    let d = *df.get(t.as_str()).unwrap_or(&0) as f64;
                    let idf = ((n as f64 - d + 0.5) / (d + 0.5) + 1.0).ln();
                    score +=
                        idf * (tf * (k1 + 1.0)) / (tf + k1 * (1.0 - b + b * dl / avg_len.max(1.0)));
                }
                (id, score as f32)
            })
            .collect();

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
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

#[cfg(target_arch = "wasm32")]
#[async_trait::async_trait(?Send)]
impl SemanticStore for InMemoryBm25 {
    async fn insert(&mut self, id: u32, load: &dyn RecordLoad) -> Result<(), String> {
        let text = load
            .text(id)
            .ok_or_else(|| "no text available".to_string())?;
        self.ids.push(id);
        self.texts.push(text);
        Ok(())
    }

    async fn search(&self, query: &dyn Query, top_k: usize) -> Result<Vec<(u32, f32)>, String> {
        let qt = match query.text() {
            Some(t) if !t.trim().is_empty() => t,
            _ => return Ok(Vec::new()),
        };
        if self.ids.is_empty() {
            return Ok(Vec::new());
        }
        let terms: Vec<String> = qt
            .to_lowercase()
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        let n = self.texts.len();
        let avg_len: f64 = self
            .texts
            .iter()
            .map(|t| t.split_whitespace().count() as f64)
            .sum::<f64>()
            / n.max(1) as f64;

        let mut df: HashMap<&str, usize> = HashMap::new();
        for t in &terms {
            let count = self
                .texts
                .iter()
                .filter(|doc| doc.to_lowercase().split_whitespace().any(|w| w == t))
                .count();
            df.insert(t, count);
        }

        let k1 = 1.2;
        let b = 0.75;
        let mut scores: Vec<(u32, f32)> = self
            .ids
            .iter()
            .zip(self.texts.iter())
            .map(|(&id, doc)| {
                let dl = doc.split_whitespace().count() as f64;
                let mut score = 0.0;
                for t in &terms {
                    let tf = doc
                        .to_lowercase()
                        .split_whitespace()
                        .filter(|w| w == t)
                        .count() as f64;
                    if tf == 0.0 {
                        continue;
                    }
                    let d = *df.get(t.as_str()).unwrap_or(&0) as f64;
                    let idf = ((n as f64 - d + 0.5) / (d + 0.5) + 1.0).ln();
                    score +=
                        idf * (tf * (k1 + 1.0)) / (tf + k1 * (1.0 - b + b * dl / avg_len.max(1.0)));
                }
                (id, score as f32)
            })
            .collect();

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
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

    fn make_store() -> InMemoryBm25 {
        InMemoryBm25::new()
    }

    #[tokio::test]
    async fn test_search_exact_match() {
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
    }

    #[tokio::test]
    async fn test_top_k_limits() {
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
    }

    #[tokio::test]
    async fn test_empty_store() {
        let store = make_store();
        assert!(store.is_empty());
    }

    #[tokio::test]
    async fn test_remove() {
        let mut store = make_store();
        store
            .insert(
                42,
                &TestLoad {
                    text: "something".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(store.len(), 1);
        store.remove(42).await.unwrap();
        assert!(store.is_empty());
    }
}
