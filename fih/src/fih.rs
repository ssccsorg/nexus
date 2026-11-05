use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

pub use crate::id::CoordId;
use crate::id::FihId20;

// ── Content-addressable identifier (demoted to content integrity only) ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FihHash(pub [u8; 32]);

impl Serialize for FihHash {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for FihHash {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let hex: String = Deserialize::deserialize(d)?;
        if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(serde::de::Error::custom(format!(
                "FihHash deserialize: expected 64 hex chars, got {}",
                hex.len()
            )));
        }
        let mut bytes = [0u8; 32];
        for i in 0..32 {
            bytes[i] = u8::from_str_radix(&hex[i * 2..=i * 2 + 1], 16)
                .map_err(|e| serde::de::Error::custom(format!("invalid hex: {e}")))?;
        }
        Ok(Self(bytes))
    }
}

impl core::fmt::Display for FihHash {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for byte in &self.0 {
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}

impl FihHash {
    pub fn new(fields: &[&str], type_tag: &str) -> Self {
        let mut h = Sha256::new();
        for f in fields {
            h.update(f.as_bytes());
        }
        h.update(type_tag.as_bytes());
        Self(h.finalize().into())
    }

    pub fn chain(a: &FihHash, b: &FihHash, c: &FihHash) -> FihHash {
        let mut h = Sha256::new();
        h.update(a.0);
        h.update(b.0);
        h.update(c.0);
        Self(h.finalize().into())
    }

    fn parse_hex_strict(hex: &str) -> Self {
        assert!(
            hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()),
            "FihHash::parse_hex_strict: expected 64 hex chars, got `{}`",
            hex
        );
        let mut bytes = [0u8; 32];
        for i in 0..32 {
            bytes[i] = u8::from_str_radix(&hex[i * 2..=i * 2 + 1], 16).expect("valid hex digit");
        }
        Self(bytes)
    }

    pub fn from_hex(hex: &str) -> Self {
        let hex_clean: String = hex.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        if hex_clean.len() == 64 {
            Self::parse_hex_strict(&hex_clean)
        } else {
            let mut h = Sha256::new();
            h.update(hex.as_bytes());
            Self(h.finalize().into())
        }
    }
}

// ── Content ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Content {
    pub mime_type: String,
    pub data: Vec<u8>,
}

impl Content {
    pub fn as_str(&self) -> Option<&str> {
        match self.mime_type.as_str() {
            "text/plain" | "application/json" => core::str::from_utf8(&self.data).ok(),
            _ => None,
        }
    }
}

impl core::fmt::Display for Content {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.mime_type.as_str() {
            "text/plain" | "application/json" => {
                if let Ok(s) = core::str::from_utf8(&self.data) {
                    write!(f, "{s}")
                } else {
                    write!(f, "<invalid utf-8 for {}>", self.mime_type)
                }
            }
            _ => write!(f, "<{}: {} bytes>", self.mime_type, self.data.len()),
        }
    }
}

impl From<String> for Content {
    fn from(s: String) -> Self {
        Content {
            mime_type: "text/plain".into(),
            data: s.into_bytes(),
        }
    }
}

impl From<&str> for Content {
    fn from(s: &str) -> Self {
        Content {
            mime_type: "text/plain".into(),
            data: s.as_bytes().to_vec(),
        }
    }
}

impl PartialEq<&str> for Content {
    fn eq(&self, other: &&str) -> bool {
        self.mime_type == "text/plain" && self.data.as_slice() == other.as_bytes()
    }
}

// ── FIH Primitives ───────────────────────────────────────────────────────

/// Fact: an immutable state snapshot.
/// `id` is a Tagma CoordPath (20 syllables, full-injective SHA-256
/// encoding since Step 4, #176) — the primary storage address.
/// `content_hash` is SHA-256 of content, used for blob dedup and integrity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fact {
    pub id: CoordId,
    pub content_hash: FihHash,
    pub origin: String,
    pub content: Content,
    pub creator: String,
}

impl Fact {
    /// Create a content-addressed Fact.
    ///
    /// The id comes from the semantic layer (`CoordId::content_fact_id`):
    /// deterministic per content + origin + creator, canonical 20-Hangul,
    /// injectively encoding the full SHA-256.
    /// `content_hash` is computed simultaneously so only one SHA-256
    /// pass is needed.
    pub fn new(origin: String, content: Content, creator: String) -> Self {
        let content_hash = {
            let Content { data, .. } = &content;
            let mut h = Sha256::new();
            h.update(data);
            FihHash(h.finalize().into())
        };
        let id = CoordId::content_fact_id(&origin, &creator, &content_hash);
        Fact {
            id,
            content_hash,
            origin,
            content,
            creator,
        }
    }

    /// Create a Fact with an explicit CoordId (opt out of content-addressed ID).
    /// Use when the caller needs a specific ID (e.g., for pre-coordinated references
    /// in tests, or when ID format is dictated by external protocol).
    pub fn with_id(id: CoordId, origin: String, content: Content, creator: String) -> Self {
        let content_hash = {
            let Content { data, .. } = &content;
            let mut h = Sha256::new();
            h.update(data);
            FihHash(h.finalize().into())
        };
        Fact {
            id,
            content_hash,
            origin,
            content,
            creator,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub id: CoordId,
    pub from_facts: Vec<CoordId>,
    pub to_fact_id: Option<CoordId>,
    pub description: String,
    pub creator: String,
    pub worker: Option<String>,
    /// The last heartbeat, in seconds since the UNIX epoch.
    pub last_heartbeat_at: Option<u64>,
    /// When the intent was created, in seconds since the UNIX epoch.
    pub created_at: Option<u64>,
    pub is_concluded: bool,
    /// When the intent concluded, in seconds since the UNIX epoch.
    pub concluded_at: Option<u64>,
}

impl Intent {
    pub fn new(
        id: CoordId,
        from_facts: Vec<CoordId>,
        to_fact_id: Option<CoordId>,
        description: String,
        creator: String,
    ) -> Self {
        Intent {
            id,
            from_facts,
            to_fact_id,
            description,
            creator,
            worker: None,
            last_heartbeat_at: None,
            created_at: None,
            is_concluded: false,
            concluded_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hint {
    pub id: CoordId,
    pub content: String,
    pub creator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardState {
    pub facts: Vec<Fact>,
    pub intents: Vec<Intent>,
    pub hints: Vec<Hint>,
}
