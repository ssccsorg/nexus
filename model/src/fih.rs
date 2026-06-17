use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

// ── Content-addressable identifier ───────────────────────────────────────

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
        // Serialization always produces 64-char hex from Display.
        // Reject anything else to fail fast on data corruption.
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

impl std::fmt::Display for FihHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
        h.update(&a.0);
        h.update(&b.0);
        h.update(&c.0);
        Self(h.finalize().into())
    }

    /// Parse exactly 64 hex characters into a FihHash. Panics on invalid input.
    /// For tests, use `FihHash::from_hex` which falls back to SHA256 for short IDs.
    fn parse_hex_strict(hex: &str) -> Self {
        assert!(hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()),
            "FihHash::parse_hex_strict: expected 64 hex chars, got `{}`", hex);
        let mut bytes = [0u8; 32];
        for i in 0..32 {
            bytes[i] = u8::from_str_radix(&hex[i * 2..=i * 2 + 1], 16)
                .expect("valid hex digit");
        }
        Self(bytes)
    }

    /// Reconstruct FihHash from a hex string or a short semantic ID.
    ///
    /// If `hex` is exactly 64 lowercase hex characters, it is parsed
    /// directly into `[u8; 32]` (round-trip with `Display`).
    /// Otherwise, the input is SHA256-hashed to produce a deterministic
    /// FihHash. This allows short test IDs like `"f001"` via SHA256.
    ///
    /// For strict parsing (e.g., deserialization), use `parse_hex_strict`.
    pub fn from_hex(hex: &str) -> Self {
        let hex_clean: String = hex.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        if hex_clean.len() == 64 {
            Self::parse_hex_strict(&hex_clean)
        } else {
            // Fallback: hash the input to produce a deterministic FihHash.
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
            "text/plain" | "application/json" => std::str::from_utf8(&self.data).ok(),
            _ => None,
        }
    }
}

impl std::fmt::Display for Content {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.mime_type.as_str() {
            "text/plain" | "application/json" => {
                if let Ok(s) = std::str::from_utf8(&self.data) {
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fact {
    pub id: FihHash,
    pub origin: String,
    pub content: Content,
    pub creator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub id: FihHash,
    pub from_facts: Vec<FihHash>,
    pub to_fact_id: Option<FihHash>,
    pub description: String,
    pub creator: String,
    pub worker: Option<String>,
    pub last_heartbeat_at: Option<u64>,
    pub created_at: Option<u64>,
    pub is_concluded: bool,
    pub concluded_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hint {
    pub id: FihHash,
    pub content: String,
    pub creator: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardState {
    pub facts: Vec<Fact>,
    pub intents: Vec<Intent>,
    pub hints: Vec<Hint>,
}
