// ── Content JSON helpers ────────────────────────────────────────────────

use nexus_model::Content;

/// Extension trait for JSON operations on Content.
pub trait ContentJsonExt {
    /// Create a Content from a JSON-serializable value.
    /// Sets mime_type to "application/json".
    fn from_json<T: serde::Serialize>(value: &T) -> Content;

    /// Try to parse this Content's data as JSON.
    /// Works for both "application/json" and "text/plain" (tries UTF-8 parse).
    fn try_parse_json<T: serde::de::DeserializeOwned>(&self) -> Option<T>;
}

impl ContentJsonExt for Content {
    fn from_json<T: serde::Serialize>(value: &T) -> Content {
        Content {
            mime_type: "application/json".into(),
            data: serde_json::to_vec(value).unwrap_or_default(),
        }
    }

    fn try_parse_json<T: serde::de::DeserializeOwned>(&self) -> Option<T> {
        match self.mime_type.as_str() {
            "application/json" => serde_json::from_slice(&self.data).ok(),
            "text/plain" => {
                let s = std::str::from_utf8(&self.data).ok()?;
                serde_json::from_str(s).ok()
            }
            _ => None,
        }
    }
}
