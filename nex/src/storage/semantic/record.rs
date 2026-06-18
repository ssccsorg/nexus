// ── Pure semantic record traits — no FIH concepts ────────────────────
//
// These traits define the minimal interface for loading record data
// (content, text, feature vectors) and querying the semantic store.
// They are intentionally FIH-agnostic so that any record storage
// system can implement them.

/// Pure semantic record loader — no FIH concepts.
///
/// Used by `SemanticStore::insert()` to retrieve the data it needs
/// (feature vectors, text, etc.) for a given record.
pub trait RecordLoad {
    /// Load content bytes for a record by its index.
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
    /// should override this via a custom `RecordLoad` wrapper that calls an
    /// embedding API and caches the result.
    fn features(&self, id: u32) -> Option<Vec<f32>>;
}

/// Pure semantic query — no FIH concepts.
///
/// Used by `SemanticStore::search()`. Unlike `RecordLoad`, it carries no
/// record ID — only the query data needed to find similar records.
/// Each implementation calls only the accessor it needs (e.g.
/// `features()` for vector search, `text()` for BM25).
pub trait Query {
    /// Query as f32 feature vector.
    fn features(&self) -> Option<Vec<f32>>;

    /// Query as UTF-8 text.
    fn text(&self) -> Option<String>;
}
