// ── Pure semantic record traits — no FIH concepts ────────────────────
//
// These traits define the minimal interface for loading record data
// (content, text, feature vectors) and querying the semantic store.
// They are intentionally FIH-agnostic so that any record storage
// system can implement them.

/// Pure semantic record loader — no FIH concepts.
#[cfg(not(target_arch = "wasm32"))]
pub trait RecordLoad: Send + Sync {
    fn content(&self, id: u32) -> Option<Vec<u8>>;
    fn text(&self, id: u32) -> Option<String> {
        self.content(id)
            .and_then(|bytes| String::from_utf8(bytes).ok())
    }
    fn features(&self, id: u32) -> Option<Vec<f32>>;
}

#[cfg(target_arch = "wasm32")]
pub trait RecordLoad {
    fn content(&self, id: u32) -> Option<Vec<u8>>;
    fn text(&self, id: u32) -> Option<String> {
        self.content(id)
            .and_then(|bytes| String::from_utf8(bytes).ok())
    }
    fn features(&self, id: u32) -> Option<Vec<f32>>;
}

/// Pure semantic query — no FIH concepts.
#[cfg(not(target_arch = "wasm32"))]
pub trait Query: Send + Sync {
    fn features(&self) -> Option<Vec<f32>>;
    fn text(&self) -> Option<String>;
}

#[cfg(target_arch = "wasm32")]
pub trait Query {
    fn features(&self) -> Option<Vec<f32>>;
    fn text(&self) -> Option<String>;
}
